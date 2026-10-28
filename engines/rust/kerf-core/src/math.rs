//! Float math that `core` lacks, ported from Zig's std.math / compiler_rt (themselves musl) so every
//! target produces identical bits. Only what the engine uses.

pub const PI: f64 = core::f64::consts::PI;
const TWO_M120: f64 = f64::from_bits(0x3870_0000_0000_0000);

fn bits(x: f64) -> u64 {
    x.to_bits()
}

fn high(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

pub fn trunc(x: f64) -> f64 {
    let b = bits(x);
    let e = ((b >> 52) & 0x7ff) as i32 - 1023;
    if e >= 52 {
        return x;
    }
    if e < 0 {
        return f64::from_bits(b & (1 << 63));
    }
    f64::from_bits(b & !((1u64 << (52 - e)) - 1))
}

pub fn floor(x: f64) -> f64 {
    let t = trunc(x);
    if x < 0.0 && t != x { t - 1.0 } else { t }
}

pub fn ceil(x: f64) -> f64 {
    let t = trunc(x);
    if x > 0.0 && t != x { t + 1.0 } else { t }
}

/// Round half away from zero.
pub fn round(x: f64) -> f64 {
    let t = trunc(x);
    if (x - t).abs() >= 0.5 { t + 1.0f64.copysign(x) } else { t }
}

/// Correctly rounded square root.
pub fn sqrt(x: f64) -> f64 {
    #[cfg(target_arch = "aarch64")]
    {
        use core::arch::aarch64::{vget_lane_f64, vdup_n_f64, vsqrt_f64};
        // SAFETY: NEON is part of the aarch64 baseline; the intrinsic is IEEE correctly rounded sqrt.
        return unsafe { vget_lane_f64::<0>(vsqrt_f64(vdup_n_f64(x))) };
    }
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::{_mm_cvtsd_f64, _mm_set_sd, _mm_sqrt_sd};
        // SAFETY: SSE2 is part of the x86_64 baseline.
        return unsafe { _mm_cvtsd_f64(_mm_sqrt_sd(_mm_set_sd(x), _mm_set_sd(x))) };
    }
    #[allow(unreachable_code)]
    sqrt_soft(x)
}

#[allow(dead_code)]
fn sqrt_soft(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 || x == f64::INFINITY {
        return x;
    }
    let b = bits(x);
    let ef = ((b >> 52) & 0x7ff) as i32;
    let mut m = b & ((1 << 52) - 1);
    let mut e2;
    if ef == 0 {
        let sh = m.leading_zeros() as i32 - 11;
        m <<= sh;
        e2 = 1 - 1075 - sh;
    } else {
        m |= 1 << 52;
        e2 = ef - 1075;
    }
    if e2 & 1 != 0 {
        m <<= 1;
        e2 -= 1;
    }
    let n = (m as u128) << 56;
    let mut op = n;
    let mut res: u128 = 0;
    let mut one: u128 = 1 << 126;
    while one > op {
        one >>= 2;
    }
    while one != 0 {
        if op >= res + one {
            op -= res + one;
            res = (res >> 1) + one;
        } else {
            res >>= 1;
        }
        one >>= 2;
    }
    let r = res as u64;
    let sticky = op != 0;
    let mut q = r >> 2;
    let rem = r & 3;
    if rem > 2 || (rem == 2 && (sticky || q & 1 == 1)) {
        q += 1;
    }
    let mut ex = e2 / 2 - 26;
    if q == 1 << 53 {
        q = 1 << 52;
        ex += 1;
    }
    f64::from_bits((((ex + 52 + 1023) as u64) << 52) | (q & ((1 << 52) - 1)))
}

/// `x * 2^n`, exact like C `scalbn`.
pub fn ldexp(x: f64, n: i32) -> f64 {
    const MANT: u32 = 52;
    const MAX_BIASED: i64 = 2046;
    let mask: u64 = (1 << MANT) - 1;
    let repr = bits(x);
    let sign = repr & (1 << 63);
    if !x.is_finite() {
        return x;
    }
    let n = n as i64;
    let mut exponent = ((repr << 1) >> (MANT + 1)) as i64;
    if exponent == 0 {
        exponent += 11 - (repr << 1).leading_zeros() as i64;
    }
    if n >= 0 {
        if n > MAX_BIASED - exponent {
            return f64::from_bits(f64::INFINITY.to_bits() | sign);
        } else if exponent + n <= 0 {
            return f64::from_bits((repr << n) | sign);
        } else if exponent <= 0 {
            let mut result = ((n + exponent) as u64) << MANT;
            result |= (repr << (1 - exponent)) & mask;
            return f64::from_bits(result | sign);
        }
        return f64::from_bits(repr + ((n as u64) << MANT));
    }
    if n <= -exponent {
        if n < -(MANT as i64 + exponent) {
            return f64::from_bits(sign);
        }
        let shift = (-n).min(-(exponent + n) + 1) as u32;
        let exact_tie = repr.trailing_zeros() == shift - 1;
        let mut result = repr & mask;
        result |= ((exponent > 0) as u64) << MANT;
        result >>= shift - 1;
        result = ((result + 1) >> 1) & !(exact_tie as u64);
        return f64::from_bits(result | sign);
    }
    f64::from_bits(repr - (((-n) as u64) << MANT))
}

/// Fused multiply-add, correctly rounded (`a * b + c`).
pub fn fma(a: f64, b: f64, c: f64) -> f64 {
    #[cfg(target_arch = "aarch64")]
    {
        use core::arch::aarch64::{vfma_f64, vget_lane_f64, vdup_n_f64};
        // SAFETY: NEON is part of the aarch64 baseline; vfma computes c + a*b with a single rounding.
        return unsafe { vget_lane_f64::<0>(vfma_f64(vdup_n_f64(c), vdup_n_f64(a), vdup_n_f64(b))) };
    }
    #[allow(unreachable_code)]
    fma_soft(a, b, c)
}

struct Unpacked {
    neg: bool,
    m: u64,
    e: i32,
}

fn unpack(x: f64) -> Unpacked {
    let b = bits(x);
    let ef = ((b >> 52) & 0x7ff) as i32;
    let frac = b & ((1 << 52) - 1);
    let (m, e) = if ef == 0 { (frac, -1074) } else { (frac | (1 << 52), ef - 1075) };
    Unpacked { neg: b >> 63 != 0, m, e }
}

#[allow(dead_code)]
fn fma_soft(a: f64, b: f64, c: f64) -> f64 {
    if !a.is_finite() || !b.is_finite() || !c.is_finite() {
        return a * b + c;
    }
    let (ua, ub, uc) = (unpack(a), unpack(b), unpack(c));
    let pneg = ua.neg != ub.neg;
    let pm = (ua.m as u128) * (ub.m as u128);
    if pm == 0 || uc.m == 0 {
        if pm == 0 && uc.m == 0 {
            return a * b + c;
        }
        if pm == 0 {
            return c;
        }
        return a * b;
    }
    // Put both operands' top bit at bit 124 so the sum has headroom.
    let ps = pm.leading_zeros() as i32 - 3;
    let pn = pm << ps;
    let pe = ua.e + ub.e - ps;
    let cs = (uc.m as u128).leading_zeros() as i32 - 3;
    let cn = (uc.m as u128) << cs;
    let ce = uc.e - cs;
    let (big, bneg, be, mut small, sneg, se) =
        if pe >= ce { (pn, pneg, pe, cn, uc.neg, ce) } else { (cn, uc.neg, ce, pn, pneg, pe) };
    let d = be - se;
    let mut sticky = false;
    if d >= 127 {
        sticky = small != 0;
        small = 0;
    } else if d > 0 {
        sticky = small & ((1u128 << d) - 1) != 0;
        small >>= d;
    }
    let (mag, neg) = if bneg == sneg {
        (big + small, bneg)
    } else if big >= small {
        (big - small - sticky as u128, bneg)
    } else {
        (small - big, sneg)
    };
    if mag == 0 && !sticky {
        return 0.0;
    }
    let top = 127 - mag.leading_zeros() as i32;
    let shift = top - 63;
    let (m64, lost) = if shift > 0 {
        ((mag >> shift) as u64, mag & ((1u128 << shift) - 1) != 0)
    } else {
        ((mag << -shift) as u64, false)
    };
    let e = be + shift;
    let sticky = sticky || lost;
    let drop = 11.max(-1074 - e);
    let m = m64 as u128;
    let r = if drop > 64 {
        0
    } else {
        let q = m >> drop;
        let rem = m & ((1u128 << drop) - 1);
        let half = 1u128 << (drop - 1);
        if rem > half || (rem == half && (sticky || q & 1 == 1)) { q + 1 } else { q }
    };
    let res = ldexp(r as f64, e + drop);
    if neg { -res } else { res }
}

pub fn hypot(x: f64, y: f64) -> f64 {
    let lower = sqrt(f64::MIN_POSITIVE);
    let upper = sqrt(f64::MAX / 2.0);
    let scale = 5e-324 * upper;
    let mut major = x;
    let mut minor = y;
    if major.is_infinite() || minor.is_infinite() {
        return f64::INFINITY;
    }
    if major.is_nan() || minor.is_nan() {
        return f64::NAN;
    }
    major = major.abs();
    minor = minor.abs();
    if minor > major {
        core::mem::swap(&mut major, &mut minor);
    }
    if minor == 0.0 {
        return major;
    }
    if major - minor == major {
        return major;
    }
    if major > upper {
        return hypot_fused(major * scale, minor * scale) / scale;
    }
    if minor < lower {
        return hypot_fused(major / scale, minor / scale) * scale;
    }
    hypot_fused(major, minor)
}

fn hypot_fused(x: f64, y: f64) -> f64 {
    let r = sqrt(fma(x, x, y * y));
    let rr = r * r;
    let xx = x * x;
    let z = fma(-y, y, rr - xx) + fma(r, r, -rr) - fma(x, x, -xx);
    r - z / (2.0 * r)
}

pub fn atan(x: f64) -> f64 {
    const ATANHI: [f64; 4] = [
        4.63647609000806093515e-01,
        7.85398163397448278999e-01,
        9.82793723247329054082e-01,
        1.57079632679489655800e+00,
    ];
    const ATANLO: [f64; 4] = [
        2.26987774529616870924e-17,
        3.06161699786838301793e-17,
        1.39033110312309984516e-17,
        6.12323399573676603587e-17,
    ];
    const AT: [f64; 11] = [
        3.33333333333329318027e-01,
        -1.99999999998764832476e-01,
        1.42857142725034663711e-01,
        -1.11111104054623557880e-01,
        9.09088713343650656196e-02,
        -7.69187620504482999495e-02,
        6.66107313738753120669e-02,
        -5.83357013379057348645e-02,
        4.97687799461593236017e-02,
        -3.65315727442169155270e-02,
        1.62858201153657823623e-02,
    ];
    let hx = bits(x);
    let ix = ((hx >> 32) & 0x7fffffff) as u32;
    let sign = (hx >> 63) != 0;
    if ix >= 0x44100000 {
        if x.is_nan() {
            return x;
        }
        let z = ATANHI[3] + TWO_M120;
        return if sign { -z } else { z };
    }
    let (xr, id): (f64, Option<usize>) = if ix < 0x3fdc_0000 {
        if ix < 0x3e40_0000 {
            return x;
        }
        (x, None)
    } else {
        let xa = x.abs();
        if ix < 0x3ff3_0000 {
            if ix < 0x3fe6_0000 {
                ((2.0 * xa - 1.0) / (2.0 + xa), Some(0))
            } else {
                ((xa - 1.0) / (xa + 1.0), Some(1))
            }
        } else if ix < 0x4003_8000 {
            ((xa - 1.5) / (1.0 + 1.5 * xa), Some(2))
        } else {
            (-1.0 / xa, Some(3))
        }
    };
    let z = xr * xr;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    match id {
        Some(i) => {
            let z_ = ATANHI[i] - (xr * (s1 + s2) - ATANLO[i] - xr);
            if sign { -z_ } else { z_ }
        }
        None => xr - xr * (s1 + s2),
    }
}

pub fn atan2(y: f64, x: f64) -> f64 {
    const PI_: f64 = 3.1415926535897931160E+00;
    const PI_LO: f64 = 1.2246467991473531772E-16;
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    let ux = bits(x);
    let mut ix = (ux >> 32) as u32;
    let lx = (ux & 0xFFFFFFFF) as u32;
    let uy = bits(y);
    let mut iy = (uy >> 32) as u32;
    let ly = (uy & 0xFFFFFFFF) as u32;
    if ix.wrapping_sub(0x3FF00000) | lx == 0 {
        return atan(y);
    }
    let m = ((iy >> 31) & 1) | ((ix >> 30) & 2);
    ix &= 0x7FFFFFFF;
    iy &= 0x7FFFFFFF;
    if iy | ly == 0 {
        return match m {
            0 | 1 => y,
            2 => PI_,
            _ => -PI_,
        };
    }
    if ix | lx == 0 {
        return if m & 1 != 0 { -PI_ / 2.0 } else { PI_ / 2.0 };
    }
    if ix == 0x7FF00000 {
        if iy == 0x7FF00000 {
            return match m {
                0 => PI_ / 4.0,
                1 => -PI_ / 4.0,
                2 => 3.0 * PI_ / 4.0,
                _ => -3.0 * PI_ / 4.0,
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI_,
            _ => -PI_,
        };
    }
    if ix.wrapping_add(64 << 20) < iy || iy == 0x7FF00000 {
        return if m & 1 != 0 { -PI_ / 2.0 } else { PI_ / 2.0 };
    }
    let z = if (m & 2) != 0 && iy.wrapping_add(64 << 20) < ix { 0.0 } else { atan((y / x).abs()) };
    match m {
        0 => z,
        1 => -z,
        2 => PI_ - (z - PI_LO),
        _ => (z - PI_LO) - PI_,
    }
}

fn rational_approx(z: f64) -> f64 {
    const PS0: f64 = 1.66666666666666657415e-01;
    const PS1: f64 = -3.25565818622400915405e-01;
    const PS2: f64 = 2.01212532134862925881e-01;
    const PS3: f64 = -4.00555345006794114027e-02;
    const PS4: f64 = 7.91534994289814532176e-04;
    const PS5: f64 = 3.47933107596021167570e-05;
    const QS1: f64 = -2.40339491173441421878e+00;
    const QS2: f64 = 2.02094576023350569471e+00;
    const QS3: f64 = -6.88283971605453293030e-01;
    const QS4: f64 = 7.70381505559019352791e-02;
    let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
    let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
    p / q
}

pub fn acos(x: f64) -> f64 {
    const PIO2_HI: f64 = 1.57079632679489655800e+00;
    const PIO2_LO: f64 = 6.12323399573676603587e-17;
    let tiny = TWO_M120;
    let hx = high(x);
    let ix = hx & 0x7fff_ffff;
    if ix >= 0x3ff0_0000 {
        let lx = bits(x) as u32;
        if (ix - 0x3ff0_0000) | lx == 0 {
            if hx >> 31 != 0 {
                return 2.0 * PIO2_HI + tiny;
            }
            return 0.0;
        }
        return f64::NAN;
    }
    if ix < 0x3fe0_0000 {
        if ix <= 0x3c60_0000 {
            return PIO2_HI + tiny;
        }
        return PIO2_HI - (x - (PIO2_LO - x * rational_approx(x * x)));
    }
    if hx >> 31 != 0 {
        let z = (1.0 + x) * 0.5;
        let s = sqrt(z);
        let w = rational_approx(z) * s - PIO2_LO;
        return 2.0 * (PIO2_HI - (s + w));
    }
    let z = (1.0 - x) * 0.5;
    let s = sqrt(z);
    let df = f64::from_bits(bits(s) & 0xffff_ffff_0000_0000);
    let c = (z - df * df) / (s + df);
    let w = rational_approx(z) * s + c;
    2.0 * (df + w)
}

fn k_cos(x: f64, y: f64) -> f64 {
    const C1: f64 = 4.16666666666666019037e-02;
    const C2: f64 = -1.38888888888741095749e-03;
    const C3: f64 = 2.48015872894767294178e-05;
    const C4: f64 = -2.75573143513906633035e-07;
    const C5: f64 = 2.08757232129817482790e-09;
    const C6: f64 = -1.13596475577881948265e-11;
    let z = x * x;
    let zs = z * z;
    let r = z * (C1 + z * (C2 + z * C3)) + zs * zs * (C4 + z * (C5 + z * C6));
    let hz = 0.5 * z;
    let w = 1.0 - hz;
    w + (((1.0 - w) - hz) + (z * r - x * y))
}

fn k_sin(x: f64, y: f64, iy: i32) -> f64 {
    const S1: f64 = -1.66666666666666324348e-01;
    const S2: f64 = 8.33333333332248946124e-03;
    const S3: f64 = -1.98412698298579493134e-04;
    const S4: f64 = 2.75573137070700676789e-06;
    const S5: f64 = -2.50507602534068634195e-08;
    const S6: f64 = 1.58969099521155010221e-10;
    let z = x * x;
    let w = z * z;
    let r = S2 + z * (S3 + z * S4) + z * w * (S5 + z * S6);
    let v = z * x;
    if iy == 0 {
        x + v * (S1 + z * r)
    } else {
        x - ((z * (0.5 * y - v * r) - y) - v * S1)
    }
}

fn k_tan(x_: f64, y_: f64, odd: bool) -> f64 {
    const T: [f64; 13] = [
        3.33333333333334091986e-01,
        1.33333333333201242699e-01,
        5.39682539762260521377e-02,
        2.18694882948595424599e-02,
        8.86323982359930005737e-03,
        3.59207910759131235356e-03,
        1.45620945432529025516e-03,
        5.88041240820264096874e-04,
        2.46463134818469906812e-04,
        7.81794442939557092300e-05,
        7.14072491382608190305e-05,
        -1.85586374855275456654e-05,
        2.59073051863633712884e-05,
    ];
    const PIO4: f64 = 7.85398163397448278999e-01;
    const PIO4LO: f64 = 3.06161699786838301793e-17;
    let mut x = x_;
    let mut y = y_;
    let hx = high(x);
    let big = (hx & 0x7fffffff) >= 0x3FE59428;
    let mut sign = false;
    if big {
        sign = hx >> 31 != 0;
        if sign {
            x = -x;
            y = -y;
        }
        x = (PIO4 - x) + (PIO4LO - y);
        y = 0.0;
    }
    let z = x * x;
    let mut w = z * z;
    let mut r = T[1] + w * (T[3] + w * (T[5] + w * (T[7] + w * (T[9] + w * T[11]))));
    let mut v = z * (T[2] + w * (T[4] + w * (T[6] + w * (T[8] + w * (T[10] + w * T[12])))));
    let s = z * x;
    r = y + z * (s * (r + v) + y) + s * T[0];
    w = x + r;
    if big {
        let s = if odd { -1.0 } else { 1.0 };
        v = s - 2.0 * (x + (r - w * w / (w + s)));
        return if sign { -v } else { v };
    }
    if !odd {
        return w;
    }
    let w0 = f64::from_bits(bits(w) & 0xffffffff00000000);
    v = r - (w0 - x);
    let a = -1.0 / w;
    let a0 = f64::from_bits(bits(a) & 0xffffffff00000000);
    a0 + a * (1.0 + a0 * w0 + a0 * v)
}

const INVPIO2: f64 = 6.36619772367581382433e-01;
const PIO2_1: f64 = 1.57079632673412561417e+00;
const PIO2_1T: f64 = 6.07710050650619224932e-11;
const PIO2_2: f64 = 6.07710050630396597660e-11;
const PIO2_2T: f64 = 2.02226624879595063154e-21;
const PIO2_3: f64 = 2.02226624871116645580e-21;
const PIO2_3T: f64 = 8.47842766036889956997e-32;
const PIO4: f64 = 7.85398163397448278999e-01;

fn rem_medium(ix: u32, x: f64, y: &mut [f64; 2]) -> i32 {
    let toint = 1.5 / f64::EPSILON;
    let mut fnn = x * INVPIO2 + toint - toint;
    let mut n = fnn as i32;
    let mut r = x - fnn * PIO2_1;
    let mut w = fnn * PIO2_1T;
    if r - w < -PIO4 {
        n -= 1;
        fnn -= 1.0;
        r = x - fnn * PIO2_1;
        w = fnn * PIO2_1T;
    } else if r - w > PIO4 {
        n += 1;
        fnn += 1.0;
        r = x - fnn * PIO2_1;
        w = fnn * PIO2_1T;
    }
    y[0] = r - w;
    let mut ey = ((bits(y[0]) >> 52) & 0x7ff) as i32;
    let ex = (ix >> 20) as i32;
    if ex - ey > 16 {
        let t = r;
        w = fnn * PIO2_2;
        r = t - w;
        w = fnn * PIO2_2T - ((t - r) - w);
        y[0] = r - w;
        ey = ((bits(y[0]) >> 52) & 0x7ff) as i32;
        if ex - ey > 49 {
            let t = r;
            w = fnn * PIO2_3;
            r = t - w;
            w = fnn * PIO2_3T - ((t - r) - w);
            y[0] = r - w;
        }
    }
    y[1] = (r - y[0]) - w;
    n
}

fn rem_pio2(x: f64, y: &mut [f64; 2]) -> i32 {
    let ui = bits(x);
    let sign = ui >> 63 != 0;
    let ix = ((ui >> 32) & 0x7fffffff) as u32;
    if ix <= 0x400f6a7a {
        if (ix & 0xfffff) == 0x921fb {
            return rem_medium(ix, x, y);
        }
        if ix <= 0x4002d97c {
            return if !sign {
                let z = x - PIO2_1;
                y[0] = z - PIO2_1T;
                y[1] = (z - y[0]) - PIO2_1T;
                1
            } else {
                let z = x + PIO2_1;
                y[0] = z + PIO2_1T;
                y[1] = (z - y[0]) + PIO2_1T;
                -1
            };
        }
        return if !sign {
            let z = x - 2.0 * PIO2_1;
            y[0] = z - 2.0 * PIO2_1T;
            y[1] = (z - y[0]) - 2.0 * PIO2_1T;
            2
        } else {
            let z = x + 2.0 * PIO2_1;
            y[0] = z + 2.0 * PIO2_1T;
            y[1] = (z - y[0]) + 2.0 * PIO2_1T;
            -2
        };
    }
    if ix <= 0x401c463b {
        if ix <= 0x4015fdbc {
            if ix == 0x4012d97c {
                return rem_medium(ix, x, y);
            }
            return if !sign {
                let z = x - 3.0 * PIO2_1;
                y[0] = z - 3.0 * PIO2_1T;
                y[1] = (z - y[0]) - 3.0 * PIO2_1T;
                3
            } else {
                let z = x + 3.0 * PIO2_1;
                y[0] = z + 3.0 * PIO2_1T;
                y[1] = (z - y[0]) + 3.0 * PIO2_1T;
                -3
            };
        }
        if ix == 0x401921fb {
            return rem_medium(ix, x, y);
        }
        return if !sign {
            let z = x - 4.0 * PIO2_1;
            y[0] = z - 4.0 * PIO2_1T;
            y[1] = (z - y[0]) - 4.0 * PIO2_1T;
            4
        } else {
            let z = x + 4.0 * PIO2_1;
            y[0] = z + 4.0 * PIO2_1T;
            y[1] = (z - y[0]) + 4.0 * PIO2_1T;
            -4
        };
    }
    if ix < 0x413921fb {
        return rem_medium(ix, x, y);
    }
    if ix >= 0x7ff00000 {
        y[0] = x - x;
        y[1] = y[0];
        return 0;
    }
    let mut ui = bits(x);
    ui &= u64::MAX >> 12;
    ui |= (0x3ff + 23) << 52;
    let mut z = f64::from_bits(ui);
    let mut tx = [0.0f64; 3];
    let mut ty = [0.0f64; 2];
    let mut i = 0usize;
    while i < 2 {
        tx[i] = (z as i32) as f64;
        z = (z - tx[i]) * 16777216.0;
        i += 1;
    }
    tx[i] = z;
    while tx[i] == 0.0 {
        i -= 1;
    }
    let n = rem_pio2_large(&tx, &mut ty, (ix >> 20) as i32 - (0x3ff + 23), i as i32 + 1);
    if sign {
        y[0] = -ty[0];
        y[1] = -ty[1];
        return -n;
    }
    y[0] = ty[0];
    y[1] = ty[1];
    n
}

const PIO2_TAB: [f64; 8] = [
    1.57079625129699707031e+00,
    7.54978941586159635335e-08,
    5.39030252995776476554e-15,
    3.28200341580791294123e-22,
    1.27065575308067607349e-29,
    1.22933308981111328932e-36,
    2.73370053816464559624e-44,
    2.16741683877804819444e-51,
];

/// The Payne-Hanek reduction for huge arguments, fixed at precision 1 (two result words, jk = 4).
fn rem_pio2_large(x: &[f64; 3], y: &mut [f64; 2], e0: i32, nx: i32) -> i32 {
    const TWO24: f64 = 16777216.0;
    const TWOM24: f64 = 1.0 / 16777216.0;
    let jk: i32 = 4;
    let jp = jk;
    let jx = nx - 1;
    let mut jv = (e0 - 3).div_euclid(24);
    if jv < 0 {
        jv = 0;
    }
    let mut q0 = e0 - 24 * (jv + 1);
    let mut iq = [0i32; 20];
    let mut f = [0.0f64; 20];
    let mut fq = [0.0f64; 20];
    let mut q = [0.0f64; 20];
    let u = |i: i32| i as usize;

    let mut j = jv - jx;
    let m = jx + jk;
    let mut i = 0;
    while i <= m {
        f[u(i)] = if j < 0 { 0.0 } else { IPIO2[u(j)] as f64 };
        i += 1;
        j += 1;
    }
    i = 0;
    while i <= jk {
        let mut fw = 0.0;
        j = 0;
        while j <= jx {
            fw += x[u(j)] * f[u(jx + i - j)];
            j += 1;
        }
        q[u(i)] = fw;
        i += 1;
    }
    let mut jz = jk;
    loop {
        i = 0;
        j = jz;
        let mut z = q[u(jz)];
        while j > 0 {
            let fw = ((TWOM24 * z) as i32) as f64;
            iq[u(i)] = (z - TWO24 * fw) as i32;
            z = q[u(j - 1)] + fw;
            i += 1;
            j -= 1;
        }
        z = ldexp(z, q0);
        z -= 8.0 * floor(z * 0.125);
        let mut n = z as i32;
        z -= n as f64;
        let mut ih = 0;
        if q0 > 0 {
            i = iq[u(jz - 1)] >> (24 - q0);
            n += i;
            iq[u(jz - 1)] -= i << (24 - q0);
            ih = iq[u(jz - 1)] >> (23 - q0);
        } else if q0 == 0 {
            ih = iq[u(jz - 1)] >> 23;
        } else if z >= 0.5 {
            ih = 2;
        }
        if ih > 0 {
            n += 1;
            let mut carry = 0;
            i = 0;
            while i < jz {
                j = iq[u(i)];
                if carry == 0 {
                    if j != 0 {
                        carry = 1;
                        iq[u(i)] = 0x1000000 - j;
                    }
                } else {
                    iq[u(i)] = 0xffffff - j;
                }
                i += 1;
            }
            if q0 > 0 {
                match q0 {
                    1 => iq[u(jz - 1)] &= 0x7fffff,
                    2 => iq[u(jz - 1)] &= 0x3fffff,
                    _ => {}
                }
            }
            if ih == 2 {
                z = 1.0 - z;
                if carry != 0 {
                    z -= ldexp(1.0, q0);
                }
            }
        }
        if z == 0.0 {
            j = 0;
            i = jz - 1;
            while i >= jk {
                j |= iq[u(i)];
                i -= 1;
            }
            if j == 0 {
                let mut k = 1;
                while iq[u(jk - k)] == 0 {
                    k += 1;
                }
                i = jz + 1;
                while i <= jz + k {
                    f[u(jx + i)] = IPIO2[u(jv + i)] as f64;
                    j = 0;
                    let mut fw = 0.0;
                    while j <= jx {
                        fw += x[u(j)] * f[u(jx + i - j)];
                        j += 1;
                    }
                    q[u(i)] = fw;
                    i += 1;
                }
                jz += k;
                continue;
            }
        }
        if z == 0.0 {
            jz -= 1;
            q0 -= 24;
            while iq[u(jz)] == 0 {
                jz -= 1;
                q0 -= 24;
            }
        } else {
            z = ldexp(z, -q0);
            if z >= TWO24 {
                let fw = ((TWOM24 * z) as i32) as f64;
                iq[u(jz)] = (z - TWO24 * fw) as i32;
                jz += 1;
                q0 += 24;
                iq[u(jz)] = fw as i32;
            } else {
                iq[u(jz)] = z as i32;
            }
        }
        let mut fw = ldexp(1.0, q0);
        i = jz;
        while i >= 0 {
            q[u(i)] = fw * iq[u(i)] as f64;
            fw *= TWOM24;
            i -= 1;
        }
        i = jz;
        while i >= 0 {
            fw = 0.0;
            let mut k = 0;
            while k <= jp && k <= jz - i {
                fw += PIO2_TAB[u(k)] * q[u(i + k)];
                k += 1;
            }
            fq[u(jz - i)] = fw;
            i -= 1;
        }
        fw = 0.0;
        i = jz;
        while i >= 0 {
            fw += fq[u(i)];
            i -= 1;
        }
        y[0] = if ih == 0 { fw } else { -fw };
        fw = fq[0] - fw;
        i = 1;
        while i <= jz {
            fw += fq[u(i)];
            i += 1;
        }
        y[1] = if ih == 0 { fw } else { -fw };
        return n & 7;
    }
}

pub fn sin(x: f64) -> f64 {
    let ix = high(x) & 0x7fffffff;
    if ix <= 0x3fe921fb {
        if ix < 0x3e500000 {
            return x;
        }
        return k_sin(x, 0.0, 0);
    }
    if ix >= 0x7ff00000 {
        return x - x;
    }
    let mut y = [0.0; 2];
    let n = rem_pio2(x, &mut y);
    match n & 3 {
        0 => k_sin(y[0], y[1], 1),
        1 => k_cos(y[0], y[1]),
        2 => -k_sin(y[0], y[1], 1),
        _ => -k_cos(y[0], y[1]),
    }
}

pub fn cos(x: f64) -> f64 {
    let ix = high(x) & 0x7fffffff;
    if ix <= 0x3fe921fb {
        if ix < 0x3e46a09e {
            return 1.0;
        }
        return k_cos(x, 0.0);
    }
    if ix >= 0x7ff00000 {
        return x - x;
    }
    let mut y = [0.0; 2];
    let n = rem_pio2(x, &mut y);
    match n & 3 {
        0 => k_cos(y[0], y[1]),
        1 => -k_sin(y[0], y[1], 1),
        2 => -k_cos(y[0], y[1]),
        _ => k_sin(y[0], y[1], 1),
    }
}

pub fn tan(x: f64) -> f64 {
    let ix = high(x) & 0x7fffffff;
    if ix <= 0x3fe921fb {
        if ix < 0x3e400000 {
            return x;
        }
        return k_tan(x, 0.0, false);
    }
    if ix >= 0x7ff00000 {
        return x - x;
    }
    let mut y = [0.0; 2];
    let n = rem_pio2(x, &mut y);
    k_tan(y[0], y[1], n & 1 != 0)
}
const IPIO2: [i32; 690] = [
    0xA2F983, 0x6E4E44, 0x1529FC, 0x2757D1, 0xF534DD, 0xC0DB62, 0x95993C, 0x439041,
    0xFE5163, 0xABDEBB, 0xC561B7, 0x246E3A, 0x424DD2, 0xE00649, 0x2EEA09, 0xD1921C,
    0xFE1DEB, 0x1CB129, 0xA73EE8, 0x8235F5, 0x2EBB44, 0x84E99C, 0x7026B4, 0x5F7E41,
    0x3991D6, 0x398353, 0x39F49C, 0x845F8B, 0xBDF928, 0x3B1FF8, 0x97FFDE, 0x05980F,
    0xEF2F11, 0x8B5A0A, 0x6D1F6D, 0x367ECF, 0x27CB09, 0xB74F46, 0x3F669E, 0x5FEA2D,
    0x7527BA, 0xC7EBE5, 0xF17B3D, 0x0739F7, 0x8A5292, 0xEA6BFB, 0x5FB11F, 0x8D5D08,
    0x560330, 0x46FC7B, 0x6BABF0, 0xCFBC20, 0x9AF436, 0x1DA9E3, 0x91615E, 0xE61B08,
    0x659985, 0x5F14A0, 0x68408D, 0xFFD880, 0x4D7327, 0x310606, 0x1556CA, 0x73A8C9,
    0x60E27B, 0xC08C6B, 0x47C419, 0xC367CD, 0xDCE809, 0x2A8359, 0xC4768B, 0x961CA6,
    0xDDAF44, 0xD15719, 0x053EA5, 0xFF0705, 0x3F7E33, 0xE832C2, 0xDE4F98, 0x327DBB,
    0xC33D26, 0xEF6B1E, 0x5EF89F, 0x3A1F35, 0xCAF27F, 0x1D87F1, 0x21907C, 0x7C246A,
    0xFA6ED5, 0x772D30, 0x433B15, 0xC614B5, 0x9D19C3, 0xC2C4AD, 0x414D2C, 0x5D000C,
    0x467D86, 0x2D71E3, 0x9AC69B, 0x006233, 0x7CD2B4, 0x97A7B4, 0xD55537, 0xF63ED7,
    0x1810A3, 0xFC764D, 0x2A9D64, 0xABD770, 0xF87C63, 0x57B07A, 0xE71517, 0x5649C0,
    0xD9D63B, 0x3884A7, 0xCB2324, 0x778AD6, 0x23545A, 0xB91F00, 0x1B0AF1, 0xDFCE19,
    0xFF319F, 0x6A1E66, 0x615799, 0x47FBAC, 0xD87F7E, 0xB76522, 0x89E832, 0x60BFE6,
    0xCDC4EF, 0x09366C, 0xD43F5D, 0xD7DE16, 0xDE3B58, 0x929BDE, 0x2822D2, 0xE88628,
    0x4D58E2, 0x32CAC6, 0x16E308, 0xCB7DE0, 0x50C017, 0xA71DF3, 0x5BE018, 0x34132E,
    0x621283, 0x014883, 0x5B8EF5, 0x7FB0AD, 0xF2E91E, 0x434A48, 0xD36710, 0xD8DDAA,
    0x425FAE, 0xCE616A, 0xA4280A, 0xB499D3, 0xF2A606, 0x7F775C, 0x83C2A3, 0x883C61,
    0x78738A, 0x5A8CAF, 0xBDD76F, 0x63A62D, 0xCBBFF4, 0xEF818D, 0x67C126, 0x45CA55,
    0x36D9CA, 0xD2A828, 0x8D61C2, 0x77C912, 0x142604, 0x9B4612, 0xC459C4, 0x44C5C8,
    0x91B24D, 0xF31700, 0xAD43D4, 0xE54929, 0x10D5FD, 0xFCBE00, 0xCC941E, 0xEECE70,
    0xF53E13, 0x80F1EC, 0xC3E7B3, 0x28F8C7, 0x940593, 0x3E71C1, 0xB3092E, 0xF3450B,
    0x9C1288, 0x7B20AB, 0x9FB52E, 0xC29247, 0x2F327B, 0x6D550C, 0x90A772, 0x1FE76B,
    0x96CB31, 0x4A1679, 0xE27941, 0x89DFF4, 0x9794E8, 0x84E6E2, 0x973199, 0x6BED88,
    0x365F5F, 0x0EFDBB, 0xB49A48, 0x6CA467, 0x427271, 0x325D8D, 0xB8159F, 0x09E5BC,
    0x25318D, 0x3974F7, 0x1C0530, 0x010C0D, 0x68084B, 0x58EE2C, 0x90AA47, 0x02E774,
    0x24D6BD, 0xA67DF7, 0x72486E, 0xEF169F, 0xA6948E, 0xF691B4, 0x5153D1, 0xF20ACF,
    0x339820, 0x7E4BF5, 0x6863B2, 0x5F3EDD, 0x035D40, 0x7F8985, 0x295255, 0xC06437,
    0x10D86D, 0x324832, 0x754C5B, 0xD4714E, 0x6E5445, 0xC1090B, 0x69F52A, 0xD56614,
    0x9D0727, 0x50045D, 0xDB3BB4, 0xC576EA, 0x17F987, 0x7D6B49, 0xBA271D, 0x296996,
    0xACCCC6, 0x5414AD, 0x6AE290, 0x89D988, 0x50722C, 0xBEA404, 0x940777, 0x7030F3,
    0x27FC00, 0xA871EA, 0x49C266, 0x3DE064, 0x83DD97, 0x973FA3, 0xFD9443, 0x8C860D,
    0xDE4131, 0x9D3992, 0x8C70DD, 0xE7B717, 0x3BDF08, 0x2B3715, 0xA0805C, 0x93805A,
    0x921110, 0xD8E80F, 0xAF806C, 0x4BFFDB, 0x0F9038, 0x761859, 0x15A562, 0xBBCB61,
    0xB989C7, 0xBD4010, 0x04F2D2, 0x277549, 0xF6B6EB, 0xBB22DB, 0xAA140A, 0x2F2689,
    0x768364, 0x333B09, 0x1A940E, 0xAA3A51, 0xC2A31D, 0xAEEDAF, 0x12265C, 0x4DC26D,
    0x9C7A2D, 0x9756C0, 0x833F03, 0xF6F009, 0x8C402B, 0x99316D, 0x07B439, 0x15200C,
    0x5BC3D8, 0xC492F5, 0x4BADC6, 0xA5CA4E, 0xCD37A7, 0x36A9E6, 0x9492AB, 0x6842DD,
    0xDE6319, 0xEF8C76, 0x528B68, 0x37DBFC, 0xABA1AE, 0x3115DF, 0xA1AE00, 0xDAFB0C,
    0x664D64, 0xB705ED, 0x306529, 0xBF5657, 0x3AFF47, 0xB9F96A, 0xF3BE75, 0xDF9328,
    0x3080AB, 0xF68C66, 0x15CB04, 0x0622FA, 0x1DE4D9, 0xA4B33D, 0x8F1B57, 0x09CD36,
    0xE9424E, 0xA4BE13, 0xB52333, 0x1AAAF0, 0xA8654F, 0xA5C1D2, 0x0F3F0B, 0xCD785B,
    0x76F923, 0x048B7B, 0x721789, 0x53A6C6, 0xE26E6F, 0x00EBEF, 0x584A9B, 0xB7DAC4,
    0xBA66AA, 0xCFCF76, 0x1D02D1, 0x2DF1B1, 0xC1998C, 0x77ADC3, 0xDA4886, 0xA05DF7,
    0xF480C6, 0x2FF0AC, 0x9AECDD, 0xBC5C3F, 0x6DDED0, 0x1FC790, 0xB6DB2A, 0x3A25A3,
    0x9AAF00, 0x9353AD, 0x0457B6, 0xB42D29, 0x7E804B, 0xA707DA, 0x0EAA76, 0xA1597B,
    0x2A1216, 0x2DB7DC, 0xFDE5FA, 0xFEDB89, 0xFDBE89, 0x6C76E4, 0xFCA906, 0x70803E,
    0x156E85, 0xFF87FD, 0x073E28, 0x336761, 0x86182A, 0xEABD4D, 0xAFE7B3, 0x6E6D8F,
    0x396795, 0x5BBF31, 0x48D784, 0x16DF30, 0x432DC7, 0x356125, 0xCE70C9, 0xB8CB30,
    0xFD6CBF, 0xA200A4, 0xE46C05, 0xA0DD5A, 0x476F21, 0xD21262, 0x845CB9, 0x496170,
    0xE0566B, 0x015299, 0x375550, 0xB7D51E, 0xC4F133, 0x5F6E13, 0xE4305D, 0xA92E85,
    0xC3B21D, 0x3632A1, 0xA4B708, 0xD4B1EA, 0x21F716, 0xE4698F, 0x77FF27, 0x80030C,
    0x2D408D, 0xA0CD4F, 0x99A520, 0xD3A2B3, 0x0A5D2F, 0x42F9B4, 0xCBDA11, 0xD0BE7D,
    0xC1DB9B, 0xBD17AB, 0x81A2CA, 0x5C6A08, 0x17552E, 0x550027, 0xF0147F, 0x8607E1,
    0x640B14, 0x8D4196, 0xDEBE87, 0x2AFDDA, 0xB6256B, 0x34897B, 0xFEF305, 0x9EBFB9,
    0x4F6A68, 0xA82A4A, 0x5AC44F, 0xBCF82D, 0x985AD7, 0x95C7F4, 0x8D4D0D, 0xA63A20,
    0x5F57A4, 0xB13F14, 0x953880, 0x0120CC, 0x86DD71, 0xB6DEC9, 0xF560BF, 0x11654D,
    0x6B0701, 0xACB08C, 0xD0C0B2, 0x485551, 0x0EFB1E, 0xC37295, 0x3B06A3, 0x3540C0,
    0x7BDC06, 0xCC45E0, 0xFA294E, 0xC8CAD6, 0x41F3E8, 0xDE647C, 0xD8649B, 0x31BED9,
    0xC397A4, 0xD45877, 0xC5E369, 0x13DAF0, 0x3C3ABA, 0x461846, 0x5F7555, 0xF5BDD2,
    0xC6926E, 0x5D2EAC, 0xED440E, 0x423E1C, 0x87C461, 0xE9FD29, 0xF3D6E7, 0xCA7C22,
    0x35916F, 0xC5E008, 0x8DD7FF, 0xE26A6E, 0xC6FDB0, 0xC10893, 0x745D7C, 0xB2AD6B,
    0x9D6ECD, 0x7B723E, 0x6A11C6, 0xA9CFF7, 0xDF7329, 0xBAC9B5, 0x5100B7, 0x0DB2E2,
    0x24BA74, 0x607DE5, 0x8AD874, 0x2C150D, 0x0C1881, 0x94667E, 0x162901, 0x767A9F,
    0xBEFDFD, 0xEF4556, 0x367ED9, 0x13D9EC, 0xB9BA8B, 0xFC97C4, 0x27A831, 0xC36EF1,
    0x36C594, 0x56A8D8, 0xB5A8B4, 0x0ECCCF, 0x2D8912, 0x34576F, 0x89562C, 0xE3CE99,
    0xB920D6, 0xAA5E6B, 0x9C2A3E, 0xCC5F11, 0x4A0BFD, 0xFBF4E1, 0x6D3B8E, 0x2C86E2,
    0x84D4E9, 0xA9B4FC, 0xD1EEEF, 0xC9352E, 0x61392F, 0x442138, 0xC8D91B, 0x0AFC81,
    0x6A4AFB, 0xD81C2F, 0x84B453, 0x8C994E, 0xCC2254, 0xDC552A, 0xD6C6C0, 0x96190B,
    0xB8701A, 0x649569, 0x605A26, 0xEE523F, 0x0F117F, 0x11B5F4, 0xF5CBFC, 0x2DBC34,
    0xEEBC34, 0xCC5DE8, 0x605EDD, 0x9B8E67, 0xEF3392, 0xB817C9, 0x9B5861, 0xBC57E1,
    0xC68351, 0x103ED8, 0x4871DD, 0xDD1C2D, 0xA118AF, 0x462C21, 0xD7F359, 0x987AD9,
    0xC0549E, 0xFA864F, 0xFC0656, 0xAE79E5, 0x362289, 0x22AD38, 0xDC9367, 0xAAE855,
    0x382682, 0x9BE7CA, 0xA40D51, 0xB13399, 0x0ED7A9, 0x480569, 0xF0B265, 0xA7887F,
    0x974C88, 0x36D1F9, 0xB39221, 0x4A827B, 0x21CF98, 0xDC9F40, 0x5547DC, 0x3A74E1,
    0x42EB67, 0xDF9DFE, 0x5FD45E, 0xA4677B, 0x7AACBA, 0xA2F655, 0x23882B, 0x55BA41,
    0x086E59, 0x862A21, 0x834739, 0xE6E389, 0xD49EE5, 0x40FB49, 0xE956FF, 0xCA0F1C,
    0x8A59C5, 0x2BFA94, 0xC5C1D3, 0xCFC50F, 0xAE5ADB, 0x86C547, 0x624385, 0x3B8621,
    0x94792C, 0x876110, 0x7B4C2A, 0x1A2C80, 0x12BF43, 0x902688, 0x893C78, 0xE4C4A8,
    0x7BDBE5, 0xC23AC4, 0xEAF426, 0x8A67F7, 0xBF920D, 0x2BA365, 0xB1933D, 0x0B7CBD,
    0xDC51A4, 0x63DD27, 0xDDE169, 0x19949A, 0x9529A8, 0x28CE68, 0xB4ED09, 0x209F44,
    0xCA984E, 0x638270, 0x237C7E, 0x32B90F, 0x8EF5A7, 0xE75614, 0x08F121, 0x2A9DB5,
    0x4D7E6F, 0x5119A5, 0xABF9B5, 0xD6DF82, 0x61DD96, 0x023616, 0x9F3AC4, 0xA1A283,
    0x6DED72, 0x7A8D39, 0xA9B882, 0x5C326B, 0x5B2746, 0xED3400, 0x7700D2, 0x55F4FC,
    0x4D5901, 0x8071E0,
];
