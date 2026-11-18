use alloc::format;
use alloc::vec::Vec;

pub struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    pub fn new() -> Self {
        Writer { out: b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec(), offsets: Vec::new() }
    }

    fn put(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }

    fn begin_object(&mut self) {
        self.offsets.push(self.out.len());
        self.put(format!("{} 0 obj\n", self.offsets.len()).as_bytes());
    }

    pub fn object(&mut self, body: &[u8]) -> usize {
        self.begin_object();
        self.put(body);
        self.put(b"\nendobj\n");
        self.offsets.len()
    }

    pub fn stream_object(&mut self, data: &[u8]) -> usize {
        self.begin_object();
        self.put(format!("<< /Length {} >>\nstream\n", data.len()).as_bytes());
        self.put(data);
        self.put(b"endstream\nendobj\n");
        self.offsets.len()
    }

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
