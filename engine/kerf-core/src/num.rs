//! Checked float to integer conversion.

pub trait Int: Copy {
    const MIN_F: f64;
    const HI_F: f64;
    fn from_f64(x: f64) -> Self;
    fn to_f64(self) -> f64;
}

macro_rules! int_impl {
    ($($t:ty),*) => {$(
        impl Int for $t {
            const MIN_F: f64 = if <$t>::MIN == 0 { -1.0 } else { <$t>::MIN as f64 };
            const HI_F: f64 = if <$t>::MIN == 0 { (<$t>::MAX as f64) + 1.0 } else { -(<$t>::MIN as f64) };
            fn from_f64(x: f64) -> Self { x as $t }
            fn to_f64(self) -> f64 { self as f64 }
        }
    )*};
}
int_impl!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/// `x` truncated toward zero, or `None` when it is not finite or does not fit `T`.
pub fn to_int<T: Int>(x: f64) -> Option<T> {
    if !x.is_finite() {
        return None;
    }
    let signed = T::MIN_F < 0.0;
    if x >= T::HI_F || (if signed { x < T::MIN_F } else { x <= -1.0 }) {
        return None;
    }
    Some(T::from_f64(x))
}

/// `x` clamped to `lo..=hi` and truncated; NaN maps to `lo`.
pub fn to_int_clamped<T: Int>(x: f64, lo: T, hi: T) -> T {
    if x.is_nan() {
        return lo;
    }
    to_int(x.clamp(lo.to_f64(), hi.to_f64())).unwrap_or(hi)
}
