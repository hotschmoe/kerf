//! Wyhash (final4, as Zig's `std.hash.Wyhash`), the content part of a document's ETag.

const SECRET: [u64; 4] = [0xa0761d6478bd642f, 0xe7037ed1a0b428db, 0x8ebc6af09c88c6e3, 0x589965cc75374cc3];

fn mum(a: u64, b: u64) -> (u64, u64) {
    let x = u128::from(a).wrapping_mul(u128::from(b));
    (x as u64, (x >> 64) as u64)
}

fn mix(a: u64, b: u64) -> u64 {
    let (lo, hi) = mum(a, b);
    lo ^ hi
}

fn read(data: &[u8], at: usize, n: usize) -> u64 {
    data.get(at..at + n).map_or(0, |s| s.iter().rev().fold(0u64, |v, &b| v << 8 | u64::from(b)))
}

pub fn wyhash(seed: u64, input: &[u8]) -> u64 {
    let s0 = seed ^ mix(seed ^ SECRET[0], SECRET[1]);
    let mut state = [s0; 3];
    let (mut a, mut b);
    if input.len() <= 16 {
        if input.len() >= 4 {
            let end = input.len() - 4;
            let quarter = (input.len() >> 3) << 2;
            a = (read(input, 0, 4) << 32) | read(input, quarter, 4);
            b = (read(input, end, 4) << 32) | read(input, end - quarter, 4);
        } else if !input.is_empty() {
            let n = input.len();
            a = (u64::from(input[0]) << 16) | (u64::from(input[n >> 1]) << 8) | u64::from(input[n - 1]);
            b = 0;
        } else {
            a = 0;
            b = 0;
        }
    } else {
        let mut i = 0;
        if input.len() >= 48 {
            while i + 48 < input.len() {
                for (k, st) in state.iter_mut().enumerate() {
                    let x = read(input, i + 16 * k, 8);
                    let y = read(input, i + 16 * k + 8, 8);
                    *st = mix(x ^ SECRET[k + 1], y ^ *st);
                }
                i += 48;
            }
            state[0] ^= state[1] ^ state[2];
        }
        let rest = &input[i..];
        let mut j = 0;
        while j + 16 < rest.len() {
            state[0] = mix(read(rest, j, 8) ^ SECRET[1], read(rest, j + 8, 8) ^ state[0]);
            j += 16;
        }
        a = read(input, input.len() - 16, 8);
        b = read(input, input.len() - 8, 8);
    }
    a ^= SECRET[1];
    b ^= state[0];
    let (lo, hi) = mum(a, b);
    mix(lo ^ SECRET[0] ^ input.len() as u64, hi ^ SECRET[1])
}
