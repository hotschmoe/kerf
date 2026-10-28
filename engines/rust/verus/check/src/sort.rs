//! Differential check of the verified stable sort against the Zig engine's `sort.stable`
//! (sort_dump.zig) on generated inputs: (key, index) pairs over small and large key ranges,
//! presorted, reversed and sawtooth orders, and f64 keys, including NaN (not a strict weak
//! ordering, where the two algorithms are allowed to disagree; reported separately).

use kerf_verified::sort::stable_sort_by;
use std::process::Command;

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

enum Case {
    Pairs(Vec<(u32, u32)>),
    Floats(Vec<f64>, bool),
}

fn cases() -> Vec<Case> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut out = Vec::new();
    let lens: Vec<usize> = (0..=300).chain([1000, 1500, 4096, 20000, 70001]).collect();
    for &len in &lens {
        for (mode, range) in [1u64, 3, 50, 1 << 30, 0, 1, 2].into_iter().enumerate() {
            let v: Vec<(u32, u32)> = (0..len)
                .map(|i| {
                    let k = match mode {
                        0..=3 => rng.below(range) as u32,
                        4 => (i / 3) as u32,
                        5 => (len - i) as u32,
                        _ => (i % 97) as u32,
                    };
                    (k, i as u32)
                })
                .collect();
            out.push(Case::Pairs(v));
        }
        let f: Vec<f64> = (0..len).map(|_| rng.below(300) as f64 - 150.0).collect();
        out.push(Case::Floats(f, false));
        if len > 1 && len <= 2000 {
            let f: Vec<f64> = (0..len)
                .map(|_| if rng.below(10) == 0 { f64::NAN } else { rng.below(40) as f64 * 0.5 })
                .collect();
            out.push(Case::Floats(f, true));
        }
    }
    out
}

fn bytes(c: &Case) -> Vec<u8> {
    let mut b = vec![0u8; 8];
    match c {
        Case::Pairs(v) => v.iter().for_each(|(k, i)| {
            b.extend_from_slice(&k.to_le_bytes());
            b.extend_from_slice(&i.to_le_bytes());
        }),
        Case::Floats(v, _) => {
            b[0] = 1;
            v.iter().for_each(|x| b.extend_from_slice(&x.to_bits().to_le_bytes()));
        }
    }
    b
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn check(args: &[String]) -> bool {
    let [zig, dir] = args else {
        eprintln!("usage: kvcheck sort <sort_dump> <workdir>");
        return false;
    };
    std::fs::create_dir_all(dir).unwrap();
    let cs = cases();
    let mut paths = Vec::new();
    let mut ours = Vec::new();
    for (n, c) in cs.iter().enumerate() {
        let p = format!("{dir}/s{n}.bin");
        std::fs::write(&p, bytes(c)).unwrap();
        paths.push(p);
        let sorted = match c {
            Case::Pairs(v) => {
                let mut v = v.clone();
                stable_sort_by(&mut v, |a, b| a.0 < b.0);
                Case::Pairs(v)
            }
            Case::Floats(v, nan) => {
                let mut v = v.clone();
                stable_sort_by(&mut v, |a, b| a < b);
                Case::Floats(v, *nan)
            }
        };
        ours.push(hex(&bytes(&sorted)[8..]));
    }
    let zig_out = Command::new(zig).args(&paths).output().expect("run sort_dump");
    let zig_lines: Vec<&str> = std::str::from_utf8(&zig_out.stdout).unwrap().lines().collect();
    assert_eq!(zig_lines.len(), cs.len());
    let (mut same, mut diff, mut nan_same, mut nan_diff) = (0, 0, 0, 0);
    for ((c, z), o) in cs.iter().zip(&zig_lines).zip(&ours) {
        let eq = z == o;
        match (c, eq) {
            (Case::Floats(_, true), true) => nan_same += 1,
            (Case::Floats(_, true), false) => nan_diff += 1,
            (_, true) => same += 1,
            (_, false) => diff += 1,
        }
    }
    println!("sort  strict weak orders: {same} identical, {diff} different");
    println!("sort  f64 with NaN (not a strict weak order): {nan_same} identical, {nan_diff} different");
    diff == 0
}
