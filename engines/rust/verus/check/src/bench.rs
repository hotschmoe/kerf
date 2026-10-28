//! Speed of the verified kernels against straightforward unverified Rust (where verification
//! shaped the code) and against the Zig engine (bench.zig, same inputs). Best of 5 rounds.

use crate::baseline_json;
use crate::sort::Rng;
use kerf_verified::{json, pdf::PdfWriter, png, sort::stable_sort_by};
use std::hint::black_box;
use std::process::Command;
use std::time::Instant;

fn best<F: FnMut()>(iters: usize, mut f: F) -> f64 {
    (0..5)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..iters {
                f();
            }
            t.elapsed().as_nanos() as f64 / iters as f64
        })
        .fold(f64::INFINITY, f64::min)
}

fn zig(bin: &str, kind: &str, file: &str, iters: usize) -> f64 {
    let out = Command::new(bin).args([kind, file, &iters.to_string()]).output().unwrap();
    String::from_utf8(out.stdout).unwrap().split_whitespace().next().unwrap().parse().unwrap()
}

fn adler_deferred(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn pdf_fmt(content: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut out = Vec::new();
    let mut offs = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    for i in 1..=5 {
        offs.push(out.len());
        write!(out, "{i} 0 obj\n").unwrap();
        if i == 4 {
            write!(out, "<< /Length {} >>\nstream\n", content.len()).unwrap();
            out.extend_from_slice(content);
            out.extend_from_slice(b"endstream\n");
        }
        out.extend_from_slice(b"endobj\n");
    }
    let x = out.len();
    write!(out, "xref\n0 6\n0000000000 65535 f \n").unwrap();
    for o in &offs {
        write!(out, "{o:0>10} 00000 n \n").unwrap();
    }
    write!(out, "trailer\n<< /Size 6 /Root 1 0 R /Info 5 0 R >>\nstartxref\n{x}\n%%EOF\n").unwrap();
    out
}

fn pdf_verified(content: &[u8]) -> Vec<u8> {
    let mut w = PdfWriter::new();
    for i in 1..=5 {
        w.begin_object();
        if i == 4 {
            w.push(b"<< /Length ");
            w.push_usize(content.len());
            w.push(b" >>\nstream\n");
            w.push(content);
            w.push(b"endstream\n");
        }
        w.push(b"endobj\n");
    }
    w.finish(1, 5).unwrap()
}

fn row(name: &str, verified: f64, baseline: Option<(&str, f64)>, zig: Option<f64>) {
    let b = baseline.map(|(n, t)| format!("{n} {:>10.0} ns ({:.2}x)", t, verified / t)).unwrap_or_default();
    let z = zig.map(|t| format!("zig {:>10.0} ns ({:.2}x)", t, verified / t)).unwrap_or_default();
    println!("{name:<28} verified {verified:>10.0} ns   {b:<38} {z}");
}

pub fn run(args: &[String]) -> bool {
    let [zb, dir, seeds @ ..] = args else {
        eprintln!("usage: kvcheck bench <zig bench> <workdir> <seed.json>...");
        return false;
    };
    std::fs::create_dir_all(dir).unwrap();
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);

    for (n, iters) in [(1_000_000usize, 3usize), (1000, 2000), (16, 200_000)] {
        let pairs: Vec<(u32, u32)> = (0..n).map(|i| (rng.below(n as u64) as u32, i as u32)).collect();
        let f = format!("{dir}/pairs{n}.bin");
        std::fs::write(&f, pairs.iter().flat_map(|(k, i)| [k.to_le_bytes(), i.to_le_bytes()].concat()).collect::<Vec<u8>>()).unwrap();
        let v = best(iters, || {
            let mut x = pairs.clone();
            stable_sort_by(&mut x, |a, b| a.0 < b.0);
            black_box(&x);
        });
        let s = best(iters, || {
            let mut x = pairs.clone();
            x.sort_by(|a, b| a.0.cmp(&b.0));
            black_box(&x);
        });
        row(&format!("sort {n} (u32 key, idx)"), v, Some(("std sort_by", s)), Some(zig(zb, "sortpairs", &f, iters)));
    }
    let fl: Vec<f64> = (0..1_000_000).map(|_| rng.below(1 << 30) as f64 * 0.37).collect();
    let f = format!("{dir}/f64.bin");
    std::fs::write(&f, fl.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
    let v = best(3, || {
        let mut x = fl.clone();
        stable_sort_by(&mut x, |a, b| a < b);
        black_box(&x);
    });
    let s = best(3, || {
        let mut x = fl.clone();
        x.sort_by(|a, b| a.partial_cmp(b).unwrap());
        black_box(&x);
    });
    row("sort 1M f64", v, Some(("std sort_by", s)), Some(zig(zb, "sortf64", &f, 3)));

    let docs: Vec<Vec<u8>> = seeds.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let big = docs.iter().max_by_key(|d| d.len()).unwrap().clone();
    let mut arr = b"[".to_vec();
    for k in 0..64 {
        if k > 0 {
            arr.push(b',');
        }
        arr.extend_from_slice(&docs[k % docs.len()]);
    }
    arr.push(b']');
    for (name, doc, iters) in [("json largest detail", &big, 2000usize), ("json 64 details in an array", &arr, 40)] {
        let f = format!("{dir}/doc{}.json", doc.len());
        std::fs::write(&f, doc).unwrap();
        let v = best(iters, || {
            black_box(json::parse(black_box(doc)).is_ok());
        });
        let b = best(iters, || {
            black_box(baseline_json::parse(black_box(doc)).is_ok());
        });
        row(&format!("{name} ({} KB)", doc.len() / 1024), v, Some(("core utf8", b)), Some(zig(zb, "json", &f, iters)));
    }

    let bytes: Vec<u8> = (0..16 << 20).map(|_| rng.next() as u8).collect();
    let f = format!("{dir}/bytes.bin");
    std::fs::write(&f, &bytes).unwrap();
    let table = png::crc_table();
    let v = best(3, || {
        black_box(!png::crc_feed(&table, 0xFFFF_FFFF, black_box(&bytes)));
    });
    row("crc32 16 MiB", v, None, Some(zig(zb, "crc", &f, 3)));
    let v = best(3, || {
        black_box(png::adler32_of(black_box(&bytes)));
    });
    let b = best(3, || {
        black_box(adler_deferred(black_box(&bytes)));
    });
    row("adler32 16 MiB", v, Some(("deferred mod", b)), Some(zig(zb, "adler", &f, 3)));
    let px: Vec<u8> = (0..1600 * 1237).map(|i| if (i / 7) % 13 == 0 { 0 } else { 255 }).collect();
    let v = best(10, || {
        black_box(png::filter_image(1600, 1237, 1, black_box(&px)));
    });
    row("png filter 1600x1237 gray", v, None, None);

    let content: Vec<u8> = (0..1 << 20).map(|i| b"0123456789 m l S\n"[i % 17]).collect();
    let v = best(200, || {
        black_box(pdf_verified(black_box(&content)));
    });
    let b = best(200, || {
        black_box(pdf_fmt(black_box(&content)));
    });
    row("pdf file, 1 MiB stream", v, Some(("core::fmt", b)), None);
    true
}

pub fn json_loop(args: &[String]) -> bool {
    let doc = std::fs::read(&args[0]).unwrap();
    let n: usize = args[1].parse().unwrap();
    for _ in 0..n {
        black_box(json::parse(black_box(&doc)).is_ok());
    }
    true
}
