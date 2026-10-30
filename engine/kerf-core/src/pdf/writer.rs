//! PDF object writer with a classic cross-reference table: every xref entry is the offset of its object's `N 0 obj`
//! header, entries are 20 bytes and in order, and `startxref` points at `xref`.

use alloc::format;
use alloc::vec::Vec;

/// The file so far and the offset of each object's `N 0 obj` header.
pub struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    /// `%PDF-1.7` and the binary-comment line that marks the file as binary.
    pub fn new() -> Self {
        Writer { out: b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec(), offsets: Vec::new() }
    }

    fn put(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }

    /// Starts the next object: records its offset and writes `N 0 obj`.
    fn begin_object(&mut self) {
        self.offsets.push(self.out.len());
        self.put(format!("{} 0 obj\n", self.offsets.len()).as_bytes());
    }

    /// An object whose body is `body` (a dictionary, say). Returns its number.
    pub fn object(&mut self, body: &[u8]) -> usize {
        self.begin_object();
        self.put(body);
        self.put(b"\nendobj\n");
        self.offsets.len()
    }

    /// A stream object with only a /Length entry. Returns its number.
    pub fn stream_object(&mut self, data: &[u8]) -> usize {
        self.begin_object();
        self.put(format!("<< /Length {} >>\nstream\n", data.len()).as_bytes());
        self.put(data);
        self.put(b"endstream\nendobj\n");
        self.offsets.len()
    }

    /// Appends the xref table and trailer; `root` and `info` are object numbers. Offsets are padded to 10 digits, and an
    /// offset of 10^10 or more widens its entry.
    pub fn finish(mut self, root: usize, info: usize) -> Vec<u8> {
        let xref_at = self.out.len();
        let count = self.offsets.len() + 1;
        self.put(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
        for offset in core::mem::take(&mut self.offsets) {
            self.put(format!("{offset:010} 00000 n \n").as_bytes());
        }
        let trailer =
            format!("trailer\n<< /Size {count} /Root {root} 0 R /Info {info} 0 R >>\nstartxref\n{xref_at}\n%%EOF\n");
        self.put(trailer.as_bytes());
        self.out
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}
