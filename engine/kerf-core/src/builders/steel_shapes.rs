//! The structural steel shape table behind `steel.shape`: an AISC Steel Construction Manual (Shapes Database v16) subset
//! for residential and light commercial work. Rolled-shape dimensions are the tabulated decimal values; HSS and pipe walls
//! are the design thickness 0.93 x nominal (AISC 360 B4.2). Root fillets of angles and channels are approximations by size
//! (the database lists k only for W shapes).

use crate::math;
use crate::num;
use crate::units;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Dimensions of one shape, inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dims {
    /// Wide flange: depth, flange width, web and flange thickness, k (design) = flange thickness + root fillet.
    W { d: f64, bf: f64, tw: f64, tf: f64, k: f64 },
    /// Square or rectangular HSS: height, width, design wall.
    Rect { h: f64, b: f64, t: f64 },
    /// Round HSS or pipe: outside diameter, design wall.
    Round { od: f64, t: f64 },
    /// Angle with legs `vert` (drawn vertical) and `horiz`, thickness and root fillet radius.
    Angle { vert: f64, horiz: f64, t: f64, r: f64 },
    /// Channel (C, MC): depth, flange width, web thickness, average flange thickness, root fillet radius.
    Channel { d: f64, bf: f64, tw: f64, tf: f64, r: f64 },
    /// Plate: thickness and width.
    Plate { t: f64, w: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// Canonical designation, e.g. `W8x18`, `HSS4x4x1/4`, `PIPE3STD`, `L4x3x3/8`, `PL1/2x8`.
    pub name: String,
    pub dims: Dims,
    /// Wall, leg or plate thickness for weld and fit checks (the thinnest element of a W or channel).
    pub t_min: f64,
}

/// W shapes: name, d, bf, tw, tf, k_des.
#[allow(clippy::approx_constant)] // tabulated dimensions, not pi
const W: &[(&str, f64, f64, f64, f64, f64)] = &[
    ("W4x13", 4.16, 4.06, 0.280, 0.345, 0.595),
    ("W5x16", 5.01, 5.00, 0.240, 0.360, 0.660),
    ("W5x19", 5.15, 5.03, 0.270, 0.430, 0.730),
    ("W6x9", 5.90, 3.94, 0.170, 0.215, 0.465),
    ("W6x12", 6.03, 4.00, 0.230, 0.280, 0.530),
    ("W6x15", 5.99, 5.99, 0.230, 0.260, 0.510),
    ("W6x16", 6.28, 4.03, 0.260, 0.405, 0.655),
    ("W6x20", 6.20, 6.02, 0.260, 0.365, 0.615),
    ("W6x25", 6.38, 6.08, 0.320, 0.455, 0.705),
    ("W8x10", 7.89, 3.94, 0.170, 0.205, 0.505),
    ("W8x13", 7.99, 4.00, 0.230, 0.255, 0.555),
    ("W8x15", 8.11, 4.02, 0.245, 0.315, 0.615),
    ("W8x18", 8.14, 5.25, 0.230, 0.330, 0.630),
    ("W8x21", 8.28, 5.27, 0.250, 0.400, 0.700),
    ("W8x24", 7.93, 6.50, 0.245, 0.400, 0.700),
    ("W8x28", 8.06, 6.54, 0.285, 0.465, 0.765),
    ("W8x31", 8.00, 8.00, 0.285, 0.435, 0.829),
    ("W8x35", 8.12, 8.02, 0.310, 0.495, 0.889),
    ("W8x40", 8.25, 8.07, 0.360, 0.560, 0.954),
    ("W10x12", 9.87, 3.96, 0.190, 0.210, 0.510),
    ("W10x15", 9.99, 4.00, 0.230, 0.270, 0.570),
    ("W10x17", 10.1, 4.01, 0.240, 0.330, 0.630),
    ("W10x19", 10.2, 4.02, 0.250, 0.395, 0.695),
    ("W10x22", 10.2, 5.75, 0.240, 0.360, 0.660),
    ("W10x26", 10.3, 5.77, 0.260, 0.440, 0.740),
    ("W10x30", 10.5, 5.81, 0.300, 0.510, 0.810),
    ("W10x33", 9.73, 7.96, 0.290, 0.435, 0.935),
    ("W10x39", 9.92, 7.99, 0.315, 0.530, 1.03),
    ("W10x45", 10.1, 8.02, 0.350, 0.620, 1.12),
    ("W10x49", 10.0, 10.0, 0.340, 0.560, 1.06),
    ("W12x14", 11.9, 3.97, 0.200, 0.225, 0.525),
    ("W12x16", 12.0, 3.99, 0.220, 0.265, 0.565),
    ("W12x19", 12.2, 4.01, 0.235, 0.350, 0.650),
    ("W12x22", 12.3, 4.03, 0.260, 0.425, 0.725),
    ("W12x26", 12.2, 6.49, 0.230, 0.380, 0.680),
    ("W12x30", 12.3, 6.52, 0.260, 0.440, 0.740),
    ("W12x35", 12.5, 6.56, 0.300, 0.520, 0.820),
    ("W12x40", 11.9, 8.01, 0.295, 0.515, 1.02),
    ("W12x45", 12.1, 8.05, 0.335, 0.575, 1.08),
    ("W12x50", 12.2, 8.08, 0.370, 0.640, 1.14),
    ("W12x53", 12.1, 10.0, 0.345, 0.575, 1.08),
    ("W14x22", 13.7, 5.00, 0.230, 0.335, 0.735),
    ("W14x26", 13.9, 5.03, 0.255, 0.420, 0.820),
    ("W14x30", 13.8, 6.73, 0.270, 0.385, 0.785),
    ("W14x34", 14.0, 6.75, 0.285, 0.455, 0.855),
    ("W14x38", 14.1, 6.77, 0.310, 0.515, 0.915),
    ("W14x43", 13.7, 8.00, 0.305, 0.530, 1.12),
    ("W14x48", 13.8, 8.03, 0.340, 0.595, 1.19),
    ("W14x53", 13.9, 8.06, 0.370, 0.660, 1.25),
    ("W16x26", 15.7, 5.50, 0.250, 0.345, 0.747),
    ("W16x31", 15.9, 5.53, 0.275, 0.440, 0.842),
    ("W16x36", 15.9, 6.99, 0.295, 0.430, 0.832),
    ("W16x40", 16.0, 7.00, 0.305, 0.505, 0.907),
    ("W16x45", 16.1, 7.04, 0.345, 0.565, 0.967),
    ("W16x50", 16.3, 7.07, 0.380, 0.630, 1.03),
    ("W18x35", 17.7, 6.00, 0.300, 0.425, 0.827),
    ("W18x40", 17.9, 6.02, 0.315, 0.525, 0.927),
    ("W18x46", 18.1, 6.06, 0.360, 0.605, 1.01),
    ("W18x50", 18.0, 7.50, 0.355, 0.570, 0.972),
    ("W18x55", 18.1, 7.53, 0.390, 0.630, 1.03),
    ("W18x60", 18.2, 7.56, 0.415, 0.695, 1.10),
    ("W21x44", 20.7, 6.50, 0.350, 0.450, 0.950),
    ("W21x50", 20.8, 6.53, 0.380, 0.535, 1.04),
    ("W21x57", 21.1, 6.56, 0.405, 0.650, 1.15),
    ("W21x62", 21.0, 8.24, 0.400, 0.615, 1.12),
    ("W24x55", 23.6, 7.01, 0.395, 0.505, 1.01),
    ("W24x62", 23.7, 7.04, 0.430, 0.590, 1.09),
    ("W24x68", 23.7, 8.97, 0.415, 0.585, 1.09),
    ("W24x76", 23.9, 8.99, 0.440, 0.680, 1.18),
];

/// Square and rectangular HSS: height, width, nominal walls.
const HSS_RECT: &[(f64, f64, &[f64])] = &[
    (2.0, 2.0, &[0.125, 0.1875, 0.25]),
    (2.5, 2.5, &[0.125, 0.1875, 0.25]),
    (3.0, 3.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375]),
    (3.5, 3.5, &[0.125, 0.1875, 0.25, 0.3125, 0.375]),
    (4.0, 4.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (4.5, 4.5, &[0.125, 0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (5.0, 5.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (5.5, 5.5, &[0.125, 0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (6.0, 6.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (7.0, 7.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (8.0, 8.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (10.0, 10.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (12.0, 12.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (3.0, 2.0, &[0.125, 0.1875, 0.25]),
    (4.0, 2.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375]),
    (4.0, 3.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375]),
    (5.0, 2.0, &[0.125, 0.1875, 0.25, 0.3125]),
    (5.0, 3.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (6.0, 2.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375]),
    (6.0, 3.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (6.0, 4.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (7.0, 3.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (7.0, 5.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (8.0, 2.0, &[0.1875, 0.25, 0.3125, 0.375]),
    (8.0, 3.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (8.0, 4.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (8.0, 6.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (10.0, 4.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (10.0, 6.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (10.0, 8.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (12.0, 4.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (12.0, 6.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
    (12.0, 8.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5, 0.625]),
];

/// Round HSS: outside diameter, nominal walls.
const HSS_ROUND: &[(f64, &[f64])] = &[
    (1.9, &[0.12, 0.145, 0.188, 0.2]),
    (2.375, &[0.125, 0.154, 0.188, 0.218, 0.25]),
    (2.875, &[0.125, 0.188, 0.203, 0.25, 0.276]),
    (3.0, &[0.12, 0.134, 0.152, 0.188, 0.203, 0.216, 0.25]),
    (3.5, &[0.125, 0.188, 0.203, 0.216, 0.25, 0.3, 0.313]),
    (4.0, &[0.125, 0.188, 0.22, 0.226, 0.237, 0.25, 0.313]),
    (4.5, &[0.125, 0.188, 0.237, 0.337, 0.375]),
    (5.0, &[0.125, 0.188, 0.25, 0.258, 0.312, 0.375, 0.5]),
    (5.5, &[0.258, 0.375, 0.5]),
    (5.563, &[0.134, 0.188, 0.258, 0.375, 0.5]),
    (6.0, &[0.188, 0.25, 0.28, 0.312, 0.375, 0.5]),
    (6.625, &[0.188, 0.25, 0.28, 0.312, 0.375, 0.432, 0.5]),
    (7.0, &[0.188, 0.25, 0.312, 0.375, 0.5]),
    (8.625, &[0.188, 0.25, 0.322, 0.375, 0.5, 0.625]),
];

/// Pipe: nominal size text, outside diameter, nominal wall STD, nominal wall XS.
#[allow(clippy::approx_constant)] // tabulated dimensions, not 1/pi
const PIPE: &[(&str, f64, f64, f64)] = &[
    ("1-1/2", 1.9, 0.145, 0.2),
    ("2", 2.375, 0.154, 0.218),
    ("2-1/2", 2.875, 0.203, 0.276),
    ("3", 3.5, 0.216, 0.3),
    ("3-1/2", 4.0, 0.226, 0.318),
    ("4", 4.5, 0.237, 0.337),
    ("5", 5.563, 0.258, 0.375),
    ("6", 6.625, 0.28, 0.432),
    ("8", 8.625, 0.322, 0.5),
];

/// Angles: long leg, short leg, thicknesses.
const ANGLES: &[(f64, f64, &[f64])] = &[
    (2.0, 2.0, &[0.125, 0.1875, 0.25, 0.3125, 0.375]),
    (2.5, 2.5, &[0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (2.5, 2.0, &[0.1875, 0.25, 0.3125, 0.375]),
    (2.5, 1.5, &[0.1875, 0.25, 0.3125]),
    (3.0, 3.0, &[0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5]),
    (3.0, 2.5, &[0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5]),
    (3.0, 2.0, &[0.1875, 0.25, 0.3125, 0.375, 0.5]),
    (3.5, 3.5, &[0.25, 0.3125, 0.375, 0.4375, 0.5]),
    (3.5, 3.0, &[0.25, 0.3125, 0.375, 0.4375, 0.5]),
    (3.5, 2.5, &[0.25, 0.3125, 0.375, 0.5]),
    (4.0, 4.0, &[0.25, 0.3125, 0.375, 0.4375, 0.5, 0.625, 0.75]),
    (4.0, 3.5, &[0.25, 0.3125, 0.375, 0.5]),
    (4.0, 3.0, &[0.25, 0.3125, 0.375, 0.4375, 0.5, 0.625]),
    (5.0, 5.0, &[0.3125, 0.375, 0.4375, 0.5, 0.625, 0.75, 0.875]),
    (5.0, 3.5, &[0.25, 0.3125, 0.375, 0.5, 0.625, 0.75]),
    (5.0, 3.0, &[0.25, 0.3125, 0.375, 0.4375, 0.5]),
    (6.0, 6.0, &[0.3125, 0.375, 0.4375, 0.5, 0.5625, 0.625, 0.75, 0.875, 1.0]),
    (6.0, 4.0, &[0.3125, 0.375, 0.4375, 0.5, 0.5625, 0.625, 0.75, 0.875]),
    (6.0, 3.5, &[0.3125, 0.375, 0.5]),
    (7.0, 4.0, &[0.375, 0.4375, 0.5, 0.625, 0.75]),
    (8.0, 8.0, &[0.5, 0.5625, 0.625, 0.75, 0.875, 1.0, 1.125]),
    (8.0, 6.0, &[0.4375, 0.5, 0.5625, 0.625, 0.75, 0.875, 1.0]),
    (8.0, 4.0, &[0.4375, 0.5, 0.5625, 0.625, 0.75, 0.875, 1.0]),
];

/// Channels C and MC: name, d, bf, tw, tf (average).
const CHANNELS: &[(&str, f64, f64, f64, f64)] = &[
    ("C3x4.1", 3.0, 1.41, 0.170, 0.273),
    ("C3x5", 3.0, 1.50, 0.258, 0.273),
    ("C3x6", 3.0, 1.60, 0.356, 0.273),
    ("C4x5.4", 4.0, 1.58, 0.184, 0.296),
    ("C4x7.25", 4.0, 1.72, 0.321, 0.296),
    ("C5x6.7", 5.0, 1.75, 0.190, 0.320),
    ("C5x9", 5.0, 1.89, 0.325, 0.320),
    ("C6x8.2", 6.0, 1.92, 0.200, 0.343),
    ("C6x10.5", 6.0, 2.03, 0.314, 0.343),
    ("C6x13", 6.0, 2.16, 0.437, 0.343),
    ("C7x9.8", 7.0, 2.09, 0.210, 0.366),
    ("C7x12.25", 7.0, 2.19, 0.314, 0.366),
    ("C8x11.5", 8.0, 2.26, 0.220, 0.390),
    ("C8x13.75", 8.0, 2.34, 0.303, 0.390),
    ("C8x18.75", 8.0, 2.53, 0.487, 0.390),
    ("C9x13.4", 9.0, 2.43, 0.233, 0.413),
    ("C9x15", 9.0, 2.49, 0.285, 0.413),
    ("C10x15.3", 10.0, 2.60, 0.240, 0.436),
    ("C10x20", 10.0, 2.74, 0.379, 0.436),
    ("C10x25", 10.0, 2.89, 0.526, 0.436),
    ("C12x20.7", 12.0, 2.94, 0.282, 0.501),
    ("C12x25", 12.0, 3.05, 0.387, 0.501),
    ("C12x30", 12.0, 3.17, 0.510, 0.501),
    ("C15x33.9", 15.0, 3.40, 0.400, 0.650),
    ("C15x40", 15.0, 3.52, 0.520, 0.650),
    ("MC6x12", 6.0, 2.50, 0.310, 0.375),
    ("MC6x15.3", 6.0, 3.50, 0.340, 0.385),
    ("MC6x16.3", 6.0, 3.00, 0.375, 0.475),
    ("MC6x18", 6.0, 3.50, 0.379, 0.475),
    ("MC7x22.7", 7.0, 3.60, 0.350, 0.500),
    ("MC8x8.5", 8.0, 1.87, 0.179, 0.311),
    ("MC8x18.7", 8.0, 2.98, 0.353, 0.500),
    ("MC8x20", 8.0, 3.03, 0.400, 0.500),
    ("MC8x21.4", 8.0, 3.45, 0.375, 0.525),
    ("MC10x22", 10.0, 3.32, 0.290, 0.575),
    ("MC10x25", 10.0, 3.41, 0.380, 0.575),
    ("MC10x28.5", 10.0, 3.95, 0.425, 0.575),
    ("MC12x10.6", 12.0, 1.50, 0.190, 0.309),
    ("MC12x31", 12.0, 3.67, 0.370, 0.700),
    ("MC12x35", 12.0, 3.87, 0.465, 0.700),
];

const HSS_DESIGN: f64 = 0.93;

/// `3.5` -> `3-1/2`, `0.25` -> `1/4`, `12` -> `12` (sixteenths; falls back to the decimal).
pub fn frac_text(x: f64) -> String {
    let Some(n) = num::to_int::<i64>(math::round(x * 16.0)) else { return crate::model::fmt_num(x) };
    if (x * 16.0 - n as f64).abs() > 1e-6 {
        return crate::model::fmt_num(x);
    }
    let (whole, mut rem, mut den) = (n / 16, n % 16, 16);
    while rem != 0 && rem % 2 == 0 {
        rem /= 2;
        den /= 2;
    }
    match (whole, rem) {
        (w, 0) => format!("{w}"),
        (0, r) => format!("{r}/{den}"),
        (w, r) => format!("{w}-{r}/{den}"),
    }
}

/// Three-decimal text of a round HSS dimension (`4.000`, `0.250`).
fn dec3(x: f64) -> String {
    let n = num::to_int_clamped::<i64>(math::round(x * 1000.0), 0, i64::MAX);
    format!("{}.{:03}", n / 1000, n % 1000)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn lengths(parts: &[&str]) -> Option<Vec<f64>> {
    parts
        .iter()
        .map(|p| {
            let t = if let Some(rest) = p.strip_prefix('.') { format!("0.{rest}") } else { String::from(*p) };
            units::parse_length_str(&t).filter(|x| *x > 0.0 && units::length_in_range(*x))
        })
        .collect()
}

fn split_x(s: &str) -> Vec<&str> {
    s.split(['x', 'X']).collect()
}

fn join_names(names: &[String]) -> String {
    names.join(", ")
}

fn w_shape(row: &(&str, f64, f64, f64, f64, f64)) -> Shape {
    let &(name, d, bf, tw, tf, k) = row;
    Shape { name: String::from(name), dims: Dims::W { d, bf, tw, tf, k }, t_min: tw.min(tf) }
}

fn rect_name(h: f64, b: f64, t: f64) -> String {
    format!("HSS{}x{}x{}", frac_text(h), frac_text(b), frac_text(t))
}

fn angle_root(long: f64) -> f64 {
    match long {
        l if l <= 2.5 => 0.25,
        l if l <= 3.5 => 0.3125,
        l if l <= 4.0 => 0.375,
        l if l <= 6.0 => 0.5,
        _ => 0.5625,
    }
}

/// Names of the table entries (and the accepted forms) for `kerf catalog`.
pub fn catalog_text() -> String {
    let w: Vec<&str> = W.iter().map(|r| r.0).collect();
    let mut hss: Vec<String> = Vec::new();
    for &(h, b, ts) in HSS_RECT {
        let tl: Vec<String> = ts.iter().map(|t| frac_text(*t)).collect();
        hss.push(format!("HSS{}x{}x({})", frac_text(h), frac_text(b), tl.join("|")));
    }
    let mut round: Vec<String> = Vec::new();
    for &(od, ts) in HSS_ROUND {
        let tl: Vec<String> = ts.iter().map(|t| dec3(*t)).collect();
        round.push(format!("HSS{}x({})", dec3(od), tl.join("|")));
    }
    let pipes: Vec<&str> = PIPE.iter().map(|p| p.0).collect();
    let mut angles: Vec<String> = Vec::new();
    for &(l, s, ts) in ANGLES {
        let tl: Vec<String> = ts.iter().map(|t| frac_text(*t)).collect();
        angles.push(format!("L{}x{}x({})", frac_text(l), frac_text(s), tl.join("|")));
    }
    let ch: Vec<&str> = CHANNELS.iter().map(|c| c.0).collect();
    format!(
        "W: {}. HSS rect/square (nominal wall): {}. HSS round (OD x nominal wall): {}. PIPE<size>STD|XS, size {}. Angles: {}. Channels: {}. Plates: PL<t>x<w>, any size, e.g. PL1/2x8, PL3/4x10.",
        w.join(" "),
        join_names(&hss),
        join_names(&round),
        pipes.join(" "),
        join_names(&angles),
        ch.join(" ")
    )
}

/// Parse a designation; Err is the E_PARAM message (what was not found and the nearest table entries).
pub fn lookup(text: &str) -> Result<Shape, String> {
    let s: String = text.chars().filter(|c| *c != ' ').collect();
    let up = s.to_ascii_uppercase();
    let forms = "Forms: W8x18, HSS4x4x1/4 (rect: height x width x nominal wall), HSS4.000x0.250 (round: OD x nominal wall), PIPE3STD / PIPE3XS, L4x3x3/8 (long leg x short leg x thickness), C8x11.5, MC8x18.7, PL1/2x8 (thickness x width)";
    if let Some(rest) = up.strip_prefix("PIPE") {
        let (size, xs) = if let Some(z) = rest.strip_suffix("STD") {
            (z, false)
        } else if let Some(z) = rest.strip_suffix("XS") {
            (z, true)
        } else {
            return Err(format!("pipe \"{text}\" needs a weight class: PIPE<size>STD or PIPE<size>XS, e.g. PIPE3STD"));
        };
        let size = size.trim_end_matches(['-', ' ']);
        let want = lengths(&[size]).and_then(|v| v.first().copied());
        for &(nm, od, std, xsw) in PIPE {
            if lengths(&[nm]).and_then(|v| v.first().copied()).is_some_and(|x| want.is_some_and(|w| near(w, x))) {
                let tn = if xs { xsw } else { std };
                let t = tn * HSS_DESIGN;
                let name = format!("PIPE{nm}{}", if xs { "XS" } else { "STD" });
                return Ok(Shape { name, dims: Dims::Round { od, t }, t_min: t });
            }
        }
        let sizes: Vec<&str> = PIPE.iter().map(|p| p.0).collect();
        return Err(format!(
            "pipe size \"{size}\" is not in the table. Sizes: {} (e.g. PIPE3STD, PIPE4XS)",
            sizes.join(", ")
        ));
    }
    if let Some(rest) = up.strip_prefix("HSS") {
        let parts = split_x(rest);
        let Some(v) = lengths(&parts) else {
            return Err(format!("HSS designation \"{text}\" is malformed. {forms}"));
        };
        return match *v.as_slice() {
            [a, b, t] => {
                let (h, w) = (a.max(b), a.min(b));
                for &(th, tb, ts) in HSS_RECT {
                    if near(th, h) && near(tb, w) {
                        if let Some(tn) = ts.iter().find(|x| near(**x, t)) {
                            let td = tn * HSS_DESIGN;
                            return Ok(Shape {
                                name: rect_name(a, b, *tn),
                                dims: Dims::Rect { h: a, b, t: td },
                                t_min: td,
                            });
                        }
                        let tl: Vec<String> = ts.iter().map(|x| frac_text(*x)).collect();
                        return Err(format!(
                            "{} has no {}\" wall in the table. Walls: {}",
                            rect_name(a, b, t).rsplit_once('x').map_or(String::new(), |p| String::from(p.0)),
                            frac_text(t),
                            tl.join(", ")
                        ));
                    }
                }
                let sizes: Vec<String> =
                    HSS_RECT.iter().map(|(h, b, _)| format!("HSS{}x{}", frac_text(*h), frac_text(*b))).collect();
                Err(format!("HSS{}x{} is not in the table. Sizes: {}", frac_text(a), frac_text(b), sizes.join(", ")))
            }
            [od, t] => {
                for &(tod, ts) in HSS_ROUND {
                    if near(tod, od) {
                        if let Some(tn) = ts.iter().find(|x| near(**x, t)) {
                            let td = tn * HSS_DESIGN;
                            let name = format!("HSS{}x{}", dec3(tod), dec3(*tn));
                            return Ok(Shape { name, dims: Dims::Round { od: tod, t: td }, t_min: td });
                        }
                        let tl: Vec<String> = ts.iter().map(|x| dec3(*x)).collect();
                        return Err(format!(
                            "HSS{} has no {} wall in the table. Walls: {}",
                            dec3(tod),
                            dec3(t),
                            tl.join(", ")
                        ));
                    }
                }
                let sizes: Vec<String> = HSS_ROUND.iter().map(|(od, _)| dec3(*od)).collect();
                Err(format!("round HSS OD {} is not in the table. ODs: {}", dec3(od), sizes.join(", ")))
            }
            _ => Err(format!("HSS designation \"{text}\" is malformed. {forms}")),
        };
    }
    if let Some(rest) = up.strip_prefix("PL") {
        let parts = split_x(rest);
        return match lengths(&parts).as_deref() {
            Some(&[t, w]) => {
                Ok(Shape { name: format!("PL{}x{}", frac_text(t), frac_text(w)), dims: Dims::Plate { t, w }, t_min: t })
            }
            _ => Err(format!("plate \"{text}\" must be PL<thickness>x<width>, e.g. PL1/2x8 or PL3/4x10")),
        };
    }
    if let Some(rest) = up.strip_prefix('L') {
        let parts = split_x(rest);
        let Some(&[a, b, t]) = lengths(&parts).as_deref() else {
            return Err(format!("angle \"{text}\" must be L<long leg>x<short leg>x<thickness>, e.g. L4x3x3/8"));
        };
        let (long, short) = (a.max(b), a.min(b));
        for &(tl, ts, thk) in ANGLES {
            if near(tl, long) && near(ts, short) {
                if let Some(tt) = thk.iter().find(|x| near(**x, t)) {
                    return Ok(Shape {
                        name: format!("L{}x{}x{}", frac_text(long), frac_text(short), frac_text(*tt)),
                        dims: Dims::Angle { vert: long, horiz: short, t: *tt, r: angle_root(long) },
                        t_min: *tt,
                    });
                }
                let tl2: Vec<String> = thk.iter().map(|x| frac_text(*x)).collect();
                return Err(format!(
                    "L{}x{} has no {}\" thickness in the table. Thicknesses: {}",
                    frac_text(long),
                    frac_text(short),
                    frac_text(t),
                    tl2.join(", ")
                ));
            }
        }
        let sizes: Vec<String> =
            ANGLES.iter().map(|(l, s, _)| format!("L{}x{}", frac_text(*l), frac_text(*s))).collect();
        return Err(format!(
            "angle L{}x{} is not in the table. Sizes: {}",
            frac_text(long),
            frac_text(short),
            sizes.join(", ")
        ));
    }
    let key = up.replace('X', "x");
    if let Some(row) = W.iter().find(|r| r.0.eq_ignore_ascii_case(&key)) {
        return Ok(w_shape(row));
    }
    if let Some(&(name, d, bf, tw, tf)) = CHANNELS.iter().find(|r| r.0.eq_ignore_ascii_case(&key)) {
        let r = (1.25 * tf).min(0.4 * (bf - tw)).min(0.25 * (d - 2.0 * tf));
        return Ok(Shape { name: String::from(name), dims: Dims::Channel { d, bf, tw, tf, r }, t_min: tw.min(tf) });
    }
    let family = if key.starts_with("MC") {
        "MC"
    } else if key.starts_with('C') {
        "C"
    } else if key.starts_with('W') {
        "W"
    } else {
        return Err(format!("steel shape \"{text}\" is not recognised. {forms}"));
    };
    let depth = key.get(family.len()..).and_then(|r| r.split('x').next()).unwrap_or("");
    let prefix = format!("{family}{depth}x");
    let same: Vec<&str> = if family == "W" {
        W.iter().map(|r| r.0).filter(|n| n.starts_with(&prefix)).collect()
    } else {
        CHANNELS.iter().map(|r| r.0).filter(|n| n.starts_with(&prefix)).collect()
    };
    if !same.is_empty() {
        return Err(format!("{text} is not in the table. {family}{depth} sizes: {}", same.join(", ")));
    }
    let depths: Vec<String> = if family == "W" {
        let mut v: Vec<String> = Vec::new();
        for r in W {
            let d = String::from(r.0.split('x').next().unwrap_or(""));
            if !v.contains(&d) {
                v.push(d);
            }
        }
        v
    } else {
        let mut v: Vec<String> = Vec::new();
        for r in CHANNELS.iter().filter(|r| r.0.starts_with(family) && (family == "MC") == r.0.starts_with("MC")) {
            let d = String::from(r.0.split('x').next().unwrap_or(""));
            if !v.contains(&d) {
                v.push(d);
            }
        }
        v
    };
    Err(format!("{text} is not in the table. Depths: {} (`kerf schema steel` lists every size)", depths.join(", ")))
}
