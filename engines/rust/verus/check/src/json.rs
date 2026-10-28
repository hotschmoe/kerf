//! Differential fuzzing of the verified JSON parser against the Zig engine's json.zig
//! (json_dump.zig): mutants of the reference documents plus targeted cases, each parsed by both,
//! compared in one canonical form (numbers as f64 bits, strings as hex, errors as line:col message).

use crate::sort::Rng;
use kerf_core::json::{Value, parse};
use std::fmt::Write;
use std::process::Command;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn dump(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push('n'),
        Value::Bool(b) => out.push(if *b { 't' } else { 'f' }),
        Value::Number(x) => write!(out, "d{:016x}", x.to_bits()).unwrap(),
        Value::String(s) => write!(out, "s{}", hex(s.as_bytes())).unwrap(),
        Value::Array(xs) => {
            out.push('[');
            for x in xs {
                dump(x, out);
                out.push(',');
            }
            out.push(']');
        }
        Value::Object(ms) => {
            out.push('{');
            for m in ms {
                write!(out, "s{}:", hex(m.key.as_bytes())).unwrap();
                dump(&m.value, out);
                out.push(',');
            }
            out.push('}');
        }
    }
}

fn ours(src: &[u8]) -> String {
    match parse(src) {
        Ok(v) => {
            let mut s = String::from("OK ");
            dump(&v, &mut s);
            s
        }
        Err(e) => format!("ERR {}:{} {}", e.line, e.col, e.msg),
    }
}

const PIECES: &[&[u8]] = &[
    b"{", b"}", b"[", b"]", b"\"", b":", b",", b" ", b"\n", b"\\", b"\\u", b"\\ud83d\\ude00", b"\\ud800",
    b"\\udc00", b"\\ud800\\u0041", b"\\u00e9", b"\\u0000", b"\\x", b"0", b"-", b"-0", b"01", b"1.", b"1.5e",
    b"1e400", b"9e15", b"9.000000000000001e15", b"1e-400", b"0.1", b"123456789012345678901234567890", b"true",
    b"tru", b"null", b"nul", b"false", b"\xc3\xa9", b"\xc3", b"\xe2\x82\xac", b"\xed\xa0\x80", b"\xf0\x9f\x98\x80",
    b"\xf4\x90\x80\x80", b"\xc0\x80", b"\xff", b"\x01", b"\t", b"\xef\xbb\xbf",
];

fn number(rng: &mut Rng) -> Vec<u8> {
    let mut t = String::new();
    if rng.below(3) == 0 {
        t.push('-');
    }
    let int_digits = 1 + rng.below(18);
    for k in 0..int_digits {
        let d = rng.below(10);
        t.push(char::from(b'0' + if k == 0 && int_digits > 1 && d == 0 { 1 } else { d as u8 }));
    }
    if rng.below(2) == 0 {
        t.push('.');
        for _ in 0..1 + rng.below(20) {
            t.push(char::from(b'0' + rng.below(10) as u8));
        }
    }
    if rng.below(3) == 0 {
        t.push(if rng.below(2) == 0 { 'e' } else { 'E' });
        match rng.below(3) {
            0 => t.push('-'),
            1 => t.push('+'),
            _ => {}
        }
        for _ in 0..1 + rng.below(3) {
            t.push(char::from(b'0' + rng.below(10) as u8));
        }
    }
    t.into_bytes()
}

const ESCAPES: &[&[u8]] = &[
    b"\\n", b"\\t", b"\\\\", b"\\/", b"\\\"", b"\\b", b"\\f", b"\\r", b"\\u00e9", b"\\u0041",
    b"\\u20ac", b"\\ud83d\\ude00", b"\\uD834\\uDD1E", b"\\uFFFF", b"\xc3\xa9", b"\xe2\x82\xac", b"\xf0\x9f\x98\x80",
];

/// Edits that keep the document well-formed: numbers swapped for random literals, escapes inside
/// strings, duplicated object members.
fn mutate_valid(rng: &mut Rng, seeds: &[Vec<u8>]) -> Vec<u8> {
    let mut d = seeds[rng.below(seeds.len() as u64) as usize].clone();
    for _ in 0..1 + rng.below(6) {
        let at = rng.below(d.len().max(1) as u64) as usize;
        match rng.below(3) {
            0 => {
                if let Some(q) = d.iter().skip(at).position(|c| c.is_ascii_digit()) {
                    let a = at + q;
                    let mut b = a;
                    while b < d.len() && (d[b].is_ascii_digit() || b".eE+-".contains(&d[b])) {
                        b += 1;
                    }
                    let a = if a > 0 && d[a - 1] == b'-' { a - 1 } else { a };
                    let n = number(rng);
                    d.splice(a..b, n);
                }
            }
            1 => {
                if let Some(q) = d.iter().skip(at).position(|&c| c == b'"') {
                    let p = ESCAPES[rng.below(ESCAPES.len() as u64) as usize];
                    let a = at + q + 1;
                    if a < d.len() && d[a] != b'"' && d[a - 1] == b'"' && (a < 2 || d[a - 2] != b'\\') {
                        d.splice(a..a, p.iter().copied());
                    }
                }
            }
            _ => {
                if let Some(q) = d.iter().skip(at).position(|&c| c == b',') {
                    let a = at + q;
                    if let Some(r) = d[a + 1..].iter().position(|&c| c == b',' || c == b'}') {
                        let member: Vec<u8> = d[a..a + 1 + r].to_vec();
                        if member.iter().all(|&c| c != b'{' && c != b'[' && c != b']') {
                            d.splice(a..a, member);
                        }
                    }
                }
            }
        }
    }
    d
}

fn mutate(rng: &mut Rng, seeds: &[Vec<u8>]) -> Vec<u8> {
    if rng.below(2) == 0 {
        return mutate_valid(rng, seeds);
    }
    let mut d = seeds[rng.below(seeds.len() as u64) as usize].clone();
    for _ in 0..1 + rng.below(4) {
        let n = d.len().max(1);
        let at = rng.below(n as u64) as usize;
        match rng.below(8) {
            0 => {
                if !d.is_empty() {
                    let at = at.min(d.len() - 1);
                    d[at] = rng.below(256) as u8;
                }
            }
            1 | 2 => {
                let p = PIECES[rng.below(PIECES.len() as u64) as usize];
                let at = at.min(d.len());
                d.splice(at..at, p.iter().copied());
            }
            3 => {
                let end = (at + 1 + rng.below(16) as usize).min(d.len());
                if at < end {
                    d.drain(at..end);
                }
            }
            4 => d.truncate(at),
            5 => {
                let end = (at + 1 + rng.below(64) as usize).min(d.len());
                if at < end {
                    let chunk: Vec<u8> = d[at..end].to_vec();
                    d.splice(at..at, chunk);
                }
            }
            6 => {
                let other = &seeds[rng.below(seeds.len() as u64) as usize];
                let o = rng.below(other.len().max(1) as u64) as usize;
                let at = at.min(d.len());
                d.splice(at..at, other[o.min(other.len())..].iter().copied().take(200));
            }
            _ => {
                if let Some(q) = d.iter().skip(at).position(|&c| c == b'"') {
                    let p = PIECES[rng.below(PIECES.len() as u64) as usize];
                    let at = at + q + 1;
                    d.splice(at..at, p.iter().copied());
                }
            }
        }
    }
    d
}

fn targeted() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = Vec::new();
    for depth in [1usize, 199, 200, 201, 5000] {
        v.push([b"[".repeat(depth), b"0".to_vec(), b"]".repeat(depth)].concat());
        v.push([b"{\"a\":".repeat(depth), b"1".to_vec(), b"}".repeat(depth)].concat());
        v.push(b"[".repeat(depth));
    }
    for p in PIECES {
        v.push(p.to_vec());
        v.push([b"\"", *p, b"\""].concat());
        v.push([b"[", *p, b"]"].concat());
        v.push([b"{\"k\":", *p, b"}"].concat());
    }
    let mut dup = String::from("{");
    for i in 0..40 {
        write!(dup, "\"k{}\":{},", i % 7, i).unwrap();
    }
    dup.push_str("\"z\":0}");
    v.push(dup.into_bytes());
    v.push(b"{\"a\":1,\"b\":2,\"a\":3}".to_vec());
    v.push(b"".to_vec());
    v.push(b"   ".to_vec());
    v.push(b"\xef\xbb\xbf{}".to_vec());
    v.push(b"{} x".to_vec());
    v
}

pub fn check(args: &[String]) -> bool {
    let [zig, dir, count, seed_paths @ ..] = args else {
        eprintln!("usage: kvcheck json <json_dump> <workdir> <mutants> <seed.json>...");
        return false;
    };
    let count: usize = count.parse().unwrap();
    std::fs::create_dir_all(dir).unwrap();
    let seeds: Vec<Vec<u8>> = seed_paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut cases: Vec<Vec<u8>> = seeds.clone();
    cases.extend(targeted());
    while cases.len() < count {
        cases.push(mutate(&mut rng, &seeds));
    }
    let mut paths = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let p = format!("{dir}/j{i}.json");
        std::fs::write(&p, c).unwrap();
        paths.push(p);
    }
    let mut zig_lines: Vec<String> = Vec::new();
    for chunk in paths.chunks(2000) {
        let out = Command::new(zig).args(chunk).output().expect("run json_dump");
        zig_lines.extend(String::from_utf8(out.stdout).unwrap().lines().map(String::from));
    }
    assert_eq!(zig_lines.len(), cases.len());
    let (mut same, mut ok_docs, mut diff) = (0usize, 0usize, 0usize);
    for (i, (c, z)) in cases.iter().zip(&zig_lines).enumerate() {
        let o = ours(c);
        if &o == z {
            same += 1;
            ok_docs += o.starts_with("OK") as usize;
        } else {
            diff += 1;
            if diff <= 5 {
                println!("json DIFF case {i}:\n  zig:  {}\n  ours: {}", &z[..z.len().min(160)], &o[..o.len().min(160)]);
            }
        }
    }
    println!("json  {} inputs: {same} identical ({ok_docs} accepted, {} rejected), {diff} different", cases.len(), same - ok_docs);
    diff == 0
}
