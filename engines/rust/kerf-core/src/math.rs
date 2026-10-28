//! Float math that `core` lacks, ported from Zig's std.math / compiler_rt (musl-derived) so every target
//! produces identical bits. Placeholder signatures until the real bodies land.

pub fn sqrt(x: f64) -> f64 { let _ = x; todo!() }
pub fn floor(x: f64) -> f64 { let _ = x; todo!() }
pub fn ceil(x: f64) -> f64 { let _ = x; todo!() }
pub fn round(x: f64) -> f64 { let _ = x; todo!() }
pub fn trunc(x: f64) -> f64 { let _ = x; todo!() }
pub fn sin(x: f64) -> f64 { let _ = x; todo!() }
pub fn cos(x: f64) -> f64 { let _ = x; todo!() }
pub fn atan(x: f64) -> f64 { let _ = x; todo!() }
pub fn atan2(y: f64, x: f64) -> f64 { let _ = (y, x); todo!() }
pub fn acos(x: f64) -> f64 { let _ = x; todo!() }
pub fn hypot(x: f64, y: f64) -> f64 { let _ = (x, y); todo!() }
pub fn ldexp(x: f64, n: i32) -> f64 { let _ = (x, n); todo!() }
