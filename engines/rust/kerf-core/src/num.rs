//! Lengths, number formatting, slopes and scales (SPEC section 1 and 6.1).

use serde_json::Value;

/// Round to 1e-4 (the canonical precision).
pub fn r4(x: f64) -> f64 {
    let v = (x * 1e4).round() / 1e4;
    if v == 0.0 { 0.0 } else { v }
}

/// Canonical number text: rounded to 1e-4, shortest round-trip decimal, `-0` prints `0`.
pub fn fmt_num(x: f64) -> String {
    if !x.is_finite() {
        return "0".to_string();
    }
    let v = r4(x);
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{}", v);
    s
}

fn parse_decimal(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut seen_digit = false;
    let mut seen_dot = false;
    for (i, ch) in s.chars().enumerate() {
        match ch {
            '0'..='9' => seen_digit = true,
            '.' if !seen_dot => seen_dot = true,
            '-' | '+' if i == 0 => {}
            _ => return None,
        }
    }
    if !seen_digit {
        return None;
    }
    s.parse::<f64>().ok()
}

fn parse_fraction(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/')?;
    let n = parse_decimal(n)?;
    let d = parse_decimal(d)?;
    if d == 0.0 {
        return None;
    }
    Some(n / d)
}

/// Inches part: `7`, `7 5/8`, `7-5/8`, `4.5`, `5/8`, optional trailing `"`.
fn parse_inches_part(s: &str) -> Option<f64> {
    let s = s.trim().trim_end_matches('"').trim();
    if s.is_empty() {
        return Some(0.0);
    }
    let toks: Vec<&str> = s.split(|c: char| c == ' ' || c == '-').filter(|t| !t.is_empty()).collect();
    match toks.len() {
        1 => {
            if toks[0].contains('/') {
                parse_fraction(toks[0])
            } else {
                parse_decimal(toks[0])
            }
        }
        2 => {
            if toks[0].contains('/') || !toks[1].contains('/') {
                return None;
            }
            let w = parse_decimal(toks[0])?;
            let f = parse_fraction(toks[1])?;
            Some(w + f)
        }
        _ => None,
    }
}

/// Parse a length string into inches (SPEC section 1).
pub fn parse_length(input: &str) -> Result<f64, String> {
    let err = || {
        format!(
            "cannot parse length {:?}: use a number of inches (7.625) or text like \"7-5/8\", \"7 5/8\", \"3'-4 1/2\\\"\", \"15/32\"",
            input
        )
    };
    let mut s = input.trim();
    if s.is_empty() {
        return Err(err());
    }
    let mut neg = false;
    if let Some(r) = s.strip_prefix('-') {
        neg = true;
        s = r.trim_start();
    } else if let Some(r) = s.strip_prefix('+') {
        s = r.trim_start();
    }
    if s.starts_with('-') || s.starts_with('+') {
        return Err(err());
    }
    let total = if let Some((feet, rest)) = s.split_once('\'') {
        let f = parse_decimal(feet).ok_or_else(err)?;
        let rest = rest.trim_start().trim_start_matches('-').trim_start();
        let inch = parse_inches_part(rest).ok_or_else(err)?;
        f * 12.0 + inch
    } else {
        parse_inches_part(s).ok_or_else(err)?
    };
    Ok(if neg { -total } else { total })
}

/// Length from a JSON value (number or string).
pub fn length_of(v: &Value) -> Result<f64, String> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| "number out of range".to_string()),
        Value::String(s) => parse_length(s),
        other => Err(format!("expected a length (number of inches or text like \"7-5/8\"), got {}", type_name(other))),
    }
}

pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Inches only part formatted `7 5/8` (no quote), from a count of sixteenths (non-negative).
fn fmt_inches16(total16: i64) -> String {
    let whole = total16 / 16;
    let frac = total16 % 16;
    if frac == 0 {
        return format!("{}", whole);
    }
    let g = gcd(frac, 16);
    let (n, d) = (frac / g, 16 / g);
    if whole == 0 { format!("{}/{}", n, d) } else { format!("{} {}/{}", whole, n, d) }
}

/// Architectural feet-inches, nearest 1/16": `0"`, `7 5/8"`, `1'-0"`, `4'-1 1/2"`, `-1'-2"`.
pub fn fmt_ftin(x: f64) -> String {
    let s16 = (x.abs() * 16.0).round() as i64;
    let neg = x < 0.0 && s16 > 0;
    let feet = s16 / 192;
    let rem = s16 % 192;
    let body = if feet > 0 { format!("{}'-{}\"", feet, fmt_inches16(rem)) } else { format!("{}\"", fmt_inches16(rem)) };
    if neg { format!("-{}", body) } else { body }
}

/// Decimal inches with at most 4 places (for LLM-facing tables).
pub fn fmt_dec(x: f64) -> String {
    fmt_num(x)
}

/// Slope `"4:12"` (rise:run) to degrees. A bare number is taken as degrees.
pub fn parse_slope(v: &Value) -> Result<f64, String> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| "bad number".to_string()),
        Value::String(s) => {
            let (a, b) = s.split_once(':').ok_or_else(|| format!("slope {:?} must look like \"4:12\" (rise:run)", s))?;
            let a = parse_decimal(a).ok_or_else(|| format!("slope {:?} must look like \"4:12\"", s))?;
            let b = parse_decimal(b).ok_or_else(|| format!("slope {:?} must look like \"4:12\"", s))?;
            if b == 0.0 {
                return Err(format!("slope {:?} has zero run", s));
            }
            Ok((a / b).atan().to_degrees())
        }
        other => Err(format!("slope must be a string like \"4:12\", got {}", type_name(other))),
    }
}

/// View scale: returns the model/paper factor, or None for NTS.
pub fn parse_scale(s: &str) -> Result<Option<f64>, String> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("nts") {
        return Ok(None);
    }
    let err = || {
        format!(
            "scale {:?} not recognized: use \"1-1/2\\\"=1'-0\\\"\" style, \"1:N\" or \"NTS\" (3\"=1'-0\" 1-1/2\"=1'-0\" 1\"=1'-0\" 3/4\"=1'-0\" 1/2\"=1'-0\" 3/8\"=1'-0\" 1/4\"=1'-0\")",
            s
        )
    };
    if let Some((l, r)) = t.split_once(':') {
        let a = parse_decimal(l).ok_or_else(err)?;
        let b = parse_decimal(r).ok_or_else(err)?;
        if a <= 0.0 || b <= 0.0 {
            return Err(err());
        }
        return Ok(Some(b / a));
    }
    if let Some((l, r)) = t.split_once('=') {
        let a = parse_inches_part(l).ok_or_else(err)?;
        let b = parse_length(r).map_err(|_| err())?;
        if a <= 0.0 || b <= 0.0 {
            return Err(err());
        }
        return Ok(Some(b / a));
    }
    Err(err())
}

/// Printable scale text, e.g. `1 1/2" = 1'-0"`.
pub fn scale_text(s: &str, factor: Option<f64>) -> String {
    let t = s.trim();
    if t.eq_ignore_ascii_case("nts") || factor.is_none() {
        return "NTS".to_string();
    }
    if t.contains(':') && !t.contains('=') {
        return t.to_string();
    }
    let f = factor.unwrap();
    // paper inches per foot
    let paper = 12.0 / f;
    let s16 = (paper * 16.0).round() as i64;
    format!("{}\" = 1'-0\"", fmt_inches16(s16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths() {
        let cases: &[(&str, f64)] = &[
            ("12", 12.0),
            ("7 5/8", 7.625),
            ("7-5/8", 7.625),
            ("7-5/8\"", 7.625),
            ("3'", 36.0),
            ("3'-0\"", 36.0),
            ("3'-4 1/2\"", 40.5),
            ("3' 4.5\"", 40.5),
            ("-1'-2\"", -14.0),
            ("0.4375", 0.4375),
            ("15/32", 0.46875),
            ("  1'-0 1/4\" ", 12.25),
            ("-7/8", -0.875),
            ("0", 0.0),
        ];
        for (s, v) in cases {
            let got = parse_length(s).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert!((got - v).abs() < 1e-9, "{s}: {got} != {v}");
        }
        for bad in ["", "abc", "1/0", "3'-x", "1 2 3", "--2"] {
            assert!(parse_length(bad).is_err(), "{bad} should fail");
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(fmt_num(7.625), "7.625");
        assert_eq!(fmt_num(7.6250), "7.625");
        assert_eq!(fmt_num(0.4375), "0.4375");
        assert_eq!(fmt_num(12.0), "12");
        assert_eq!(fmt_num(-0.0), "0");
        assert_eq!(fmt_num(-0.00001), "0");
        assert_eq!(fmt_num(1.0 / 3.0), "0.3333");
        assert_eq!(fmt_num(-2.5), "-2.5");
        assert_eq!(fmt_num(97.125), "97.125");
    }

    #[test]
    fn ftin() {
        assert_eq!(fmt_ftin(0.0), "0\"");
        assert_eq!(fmt_ftin(7.625), "7 5/8\"");
        assert_eq!(fmt_ftin(12.0), "1'-0\"");
        assert_eq!(fmt_ftin(49.5), "4'-1 1/2\"");
        assert_eq!(fmt_ftin(-14.0), "-1'-2\"");
        assert_eq!(fmt_ftin(0.5), "1/2\"");
        assert_eq!(fmt_ftin(0.5 + 1.0 / 32.0), "9/16\""); // rounds to nearest 1/16
        assert_eq!(fmt_ftin(1.75), "1 3/4\"");
        assert_eq!(fmt_ftin(-0.001), "0\"");
        assert_eq!(fmt_ftin(11.99), "1'-0\"");
        assert_eq!(fmt_ftin(0.125), "1/8\"");
    }

    #[test]
    fn scales_and_slopes() {
        assert_eq!(parse_scale("1-1/2\"=1'-0\"").unwrap(), Some(8.0));
        assert_eq!(parse_scale("1\"=1'-0\"").unwrap(), Some(12.0));
        assert_eq!(parse_scale("3\"=1'-0\"").unwrap(), Some(4.0));
        assert_eq!(parse_scale("3/4\"=1'-0\"").unwrap(), Some(16.0));
        assert_eq!(parse_scale("1:20").unwrap(), Some(20.0));
        assert_eq!(parse_scale("NTS").unwrap(), None);
        assert!(parse_scale("big").is_err());
        let d = parse_slope(&Value::String("4:12".into())).unwrap();
        assert!((d - 18.4349488).abs() < 1e-6);
        assert_eq!(scale_text("1-1/2\"=1'-0\"", Some(8.0)), "1 1/2\" = 1'-0\"");
    }
}
