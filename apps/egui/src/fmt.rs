//! Architectural feet-inches formatting (SPEC §1), clock helpers.

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

/// `7 5/8"`, `1'-0"`, `4'-1 1/2"`, `-1'-2"`; rounded to the nearest 1/16".
pub fn ft_in(inches: f64) -> String {
    let neg = inches < 0.0;
    let sixteenths = (inches.abs() * 16.0).round() as i64;
    let total_in = sixteenths / 16;
    let frac16 = sixteenths % 16;
    let (feet, inch) = (total_in / 12, total_in % 12);
    let frac = if frac16 == 0 {
        String::new()
    } else {
        let g = gcd(frac16, 16);
        format!("{}/{}", frac16 / g, 16 / g)
    };
    let mut s = String::new();
    if neg && sixteenths != 0 {
        s.push('-');
    }
    if feet > 0 {
        s += &format!("{feet}'-");
        s += &inch.to_string();
        if !frac.is_empty() {
            s += &format!(" {frac}");
        }
        s.push('"');
    } else if inch == 0 && !frac.is_empty() {
        s += &format!("{frac}\"");
    } else {
        s += &inch.to_string();
        if !frac.is_empty() {
            s += &format!(" {frac}");
        }
        s.push('"');
    }
    s
}

/// `HH:MM` (local on the web, UTC natively).
pub fn clock() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let d = js_sys::Date::new_0();
        format!("{:02}:{:02}", d.get_hours(), d.get_minutes())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let secs = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        format!("{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ftin() {
        assert_eq!(ft_in(0.0), "0\"");
        assert_eq!(ft_in(7.625), "7 5/8\"");
        assert_eq!(ft_in(12.0), "1'-0\"");
        assert_eq!(ft_in(49.5), "4'-1 1/2\"");
        assert_eq!(ft_in(-14.0), "-1'-2\"");
        assert_eq!(ft_in(0.5), "1/2\"");
    }
}
