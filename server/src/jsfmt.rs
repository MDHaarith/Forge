use serde_json::{Number, Value};

/// String.trim() includes BOM and excludes NEL, unlike Rust's Unicode whitespace set.
pub fn trim(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
        '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'))
}

/// JavaScript prints negative zero as zero and uses exponent notation outside [1e-6, 1e21).
pub fn number(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    if n.abs() < 1e-6 || n.abs() >= 1e21 {
        let raw = format!("{n:e}");
        let (mantissa, exponent) = raw.split_once('e').unwrap();
        let exponent: i32 = exponent.parse().unwrap();
        return format!(
            "{mantissa}e{}{exponent}",
            if exponent >= 0 { "+" } else { "" }
        );
    }
    n.to_string()
}

/// Math.round breaks half-way ties toward positive infinity.
pub fn round(n: f64) -> f64 {
    if n.fract() == 0.0 {
        n
    } else {
        (n + 0.5).floor()
    }
}

/// Number(n.toFixed(digits)): round the actual binary value, with decimal ties away from zero.
pub fn fixed(n: f64, digits: usize) -> f64 {
    if n == 0.0 || !n.is_finite() || n.abs() >= 1e21 {
        return n + 0.0;
    }
    let bits = n.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = (bits & ((1u64 << 52) - 1)) | if exponent == 0 { 0 } else { 1u64 << 52 };
    let shift = if exponent == 0 {
        -1074
    } else {
        exponent - 1023 - 52
    } + digits as i32;
    let scaled =
        (mantissa as u128).checked_mul(5u128.checked_pow(digits as u32).unwrap_or(u128::MAX));
    let rounded = scaled.and_then(|v| {
        if shift >= 0 {
            if shift < 128 && v <= (u128::MAX >> shift) {
                Some(v << shift)
            } else {
                None
            }
        } else {
            let shift = (-shift) as u32;
            if shift >= 128 {
                Some(0)
            } else {
                let whole = v >> shift;
                let remainder = v & ((1u128 << shift) - 1);
                Some(whole + u128::from(remainder >= (1u128 << (shift - 1))))
            }
        }
    });
    let result = rounded
        .map(|v| {
            let mut decimal = format!("{v:0width$}", width = digits + 1);
            if digits > 0 {
                decimal.insert(decimal.len() - digits, '.');
            }
            decimal.parse::<f64>().unwrap()
        })
        .unwrap_or_else(|| format!("{n:.*}", digits).parse().unwrap());
    if rounded.is_some() {
        result.copysign(n) + 0.0
    } else {
        result + 0.0
    }
}

pub fn fixed_string(n: f64, digits: usize) -> String {
    number(fixed(n, digits))
}

/// Number(n.toPrecision(digits)), formatted using the shortest round-trip representation.
pub fn precision(n: f64, digits: usize) -> f64 {
    if n == 0.0 || !n.is_finite() {
        return n + 0.0;
    }
    let exponent: i32 = format!("{n:e}").split_once('e').unwrap().1.parse().unwrap();
    let places = digits as i32 - 1 - exponent;
    if (0..=32).contains(&places) {
        return fixed(n, places as usize);
    }
    if places < 0 && n.fract() == 0.0 && n.abs() < u128::MAX as f64 {
        if let Some(scale) = 10u128.checked_pow((-places) as u32) {
            let whole = n.abs() as u128;
            let remainder = whole % scale;
            let rounded = whole / scale + u128::from(remainder >= scale - remainder);
            if let Some(value) = rounded.checked_mul(scale) {
                return (value as f64).copysign(n);
            }
        }
    }
    format!("{:.*e}", digits.saturating_sub(1), n)
        .parse::<f64>()
        .unwrap()
        + 0.0
}

/// n.toPrecision(digits) keeps trailing zeroes, including in matching-network keys.
pub fn precision_string(n: f64, digits: usize) -> String {
    if !n.is_finite() {
        return number(n);
    }
    let rounded = precision(n, digits);
    let raw = format!("{:.*e}", digits - 1, rounded);
    let (mantissa, exponent) = raw.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    if exponent < -6 || exponent >= digits as i32 {
        format!("{mantissa}e{}{exponent}", if exponent >= 0 { "+" } else { "" })
    } else {
        format!("{:.*}", (digits as i32 - 1 - exponent) as usize, rounded + 0.0)
    }
}

/// String(value), including JavaScript's array and missing-value coercion.
pub fn string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => number(n.as_f64().unwrap()),
        Some(Value::Array(a)) => a.iter().map(|v| if v.is_null() { String::new() } else { string(Some(v)) }).collect::<Vec<_>>().join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// Number(string) accepts decimal exponents, unsigned radix prefixes and Infinity.
pub fn parse_number(text: &str) -> f64 {
    let text = trim(text);
    if text.is_empty() { return 0.0; }
    if matches!(text, "Infinity" | "+Infinity") { return f64::INFINITY; }
    if text == "-Infinity" { return f64::NEG_INFINITY; }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if text.get(..2).is_some_and(|v| v.eq_ignore_ascii_case(prefix)) {
            let mut out = 0.0;
            if text.len() == 2 { return f64::NAN; }
            for c in text[2..].chars() {
                let Some(digit) = c.to_digit(radix) else { return f64::NAN; };
                out = out * radix as f64 + digit as f64;
            }
            return out;
        }
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if !unsigned.bytes().any(|b| b.is_ascii_digit()) || !unsigned.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

/// Number(value). Missing is NaN, whereas null is zero.
pub fn numeric(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => u8::from(*b) as f64,
        Some(Value::Number(n)) => n.as_f64().unwrap(),
        Some(value) => parse_number(&string(Some(value))),
    }
}

pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// toLocaleString("en") groups integer digits in threes.
pub fn locale(n: f64) -> String {
    let rounded = if n.abs() >= 1e21 {
        n
    } else {
        (n * 1000.0).round() / 1000.0
    };
    let raw = if rounded == 0.0 {
        "0".into()
    } else {
        rounded.to_string()
    };
    let (integer, fraction) = raw.split_once('.').unwrap_or((&raw, ""));
    let (sign, integer) = integer
        .strip_prefix('-')
        .map_or(("", integer), |v| ("-", v));
    let mut out = sign.to_owned();
    for (i, c) in integer.chars().enumerate() {
        if i > 0 && (integer.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(fraction);
    }
    out
}

/// JSON.stringify writes whole numbers without a decimal point.
pub fn json_number(n: f64) -> Value {
    if n == 0.0 {
        return Value::from(0);
    }
    if n.fract() == 0.0 && n >= i64::MIN as f64 && n < i64::MAX as f64 {
        Value::from(n as i64)
    } else if n.fract() == 0.0 && n >= 0.0 && n < u64::MAX as f64 {
        Value::from(n as u64)
    } else {
        Number::from_f64(n)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

pub fn normalize_json(value: &mut Value) {
    match value {
        Value::Number(n) if n.is_f64() => *value = json_number(n.as_f64().unwrap()),
        Value::Array(values) => values.iter_mut().for_each(normalize_json),
        Value::Object(values) => values.values_mut().for_each(normalize_json),
        _ => {}
    }
}

/// JSON.stringify(value, null, 2), with JavaScript's number spelling.
pub fn stringify_pretty(value: &Value) -> Result<String, serde_json::Error> {
    let text = serde_json::to_string_pretty(value)?;
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i] == b'"' {
            i += 1;
            while bytes[i] != b'"' { i += if bytes[i] == b'\\' { 2 } else { 1 }; }
            i += 1;
            out.push_str(&text[start..i]);
        } else if bytes[i].is_ascii_digit() || bytes[i] == b'-' {
            i += 1;
            while i < bytes.len() && matches!(bytes[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') { i += 1; }
            out.push_str(&number(text[start..i].parse().unwrap()));
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn javascript_pretty_json_numbers_and_escaped_strings() {
        let value = serde_json::json!({"5.0": [5.0, -0.0, 1e-6, 1e-7, 1e21], "text": "é 5.0 \\\" -0.0", "empty": [], "nested": {"n": 0.1}});
        assert_eq!(stringify_pretty(&value).unwrap(), "{\n  \"5.0\": [\n    5,\n    0,\n    0.000001,\n    1e-7,\n    1e+21\n  ],\n  \"text\": \"é 5.0 \\\\\\\" -0.0\",\n  \"empty\": [],\n  \"nested\": {\n    \"n\": 0.1\n  }\n}");
        assert_eq!(stringify_pretty(&serde_json::json!(1e-6)).unwrap(), "0.000001");
    }
    #[test]
    fn javascript_numbers() {
        assert_eq!(trim("\u{feff} hello \u{feff}"), "hello");
        assert_eq!(trim("\u{0085}"), "\u{0085}");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(5.0), "5");
        assert_eq!(number(0.1), "0.1");
        assert_eq!(number(1e-7), "1e-7");
        assert_eq!(number(1e21), "1e+21");
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(4503599627370497.0), 4503599627370497.0);
        assert_eq!(fixed_string(-0.000001, 5), "0");
        assert_eq!(fixed_string(2.5, 0), "3");
        assert_eq!(fixed_string(-2.5, 0), "-3");
        assert_eq!(fixed_string(1.005, 2), "1");
        assert_eq!(fixed_string(0.03125, 4), "0.0313");
        assert_eq!(number(precision(1234.5678, 4)), "1235");
        assert_eq!(number(precision(2.5, 1)), "3");
        assert_eq!(number(precision(-2.5, 1)), "-3");
        assert_eq!(number(precision(1250.0, 2)), "1300");
        assert_eq!(number(precision(0.125, 2)), "0.13");
        assert_eq!(number(precision(2f64.powi(-22), 15)), "2.38418579101563e-7");
        assert_eq!(locale(1234567.0), "1,234,567");
        assert_eq!(locale(12.3455), "12.346");
        assert_eq!(locale(1e21), "1,000,000,000,000,000,000,000");
        assert_eq!(json_number(5.0).to_string(), "5");
    }

    #[test]
    fn javascript_precision_keys_and_coercion() {
        assert_eq!(precision_string(4.7e-9, 6), "4.70000e-9");
        assert_eq!(precision_string(0.000001, 6), "0.00000100000");
        assert_eq!(precision_string(1234567.0, 6), "1.23457e+6");
        assert_eq!(precision_string(0.0, 6), "0.00000");
        assert_eq!(precision_string(-0.0, 6), "0.00000");
        assert_eq!(precision_string(9.999999, 6), "10.0000");
        assert_eq!(string(None), "undefined");
        assert_eq!(string(Some(&serde_json::json!([1, null, [2, 3]]))), "1,,2,3");
        assert_eq!(numeric(Some(&Value::Null)), 0.0);
        assert_eq!(parse_number(" 0x10 "), 16.0);
        assert_eq!(parse_number("0b11"), 3.0);
        assert_eq!(parse_number("0o10"), 8.0);
        assert_eq!(parse_number("-1.5e2"), -150.0);
        assert_eq!(parse_number("Infinity"), f64::INFINITY);
        for s in ["inf", "infinity", "+0x10", "0x", "1 2"] { assert!(parse_number(s).is_nan()); }
    }
}
