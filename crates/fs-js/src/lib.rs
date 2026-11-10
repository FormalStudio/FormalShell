//! The JavaScript semantics the logic here leans on: `String(x)`,
//! `Number(x)`, `Math.round`, `toFixed`, `parseInt`, `trim`, `localeCompare`.
//! Output strings reach IPC and the UI byte for byte, so these match the
//! engine rather than Rust's neighbours (`f64::round` rounds half away from
//! zero, JS rounds half up; Rust's `trim` also strips U+0085, JS's does not).

pub mod lenient;

use std::cmp::Ordering;
use std::sync::LazyLock;

use icu_collator::options::CollatorOptions;
use icu_collator::{Collator, CollatorBorrowed};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

/// The whitespace set of JS `\s` and `String.prototype.trim`.
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{b}'
            | '\u{c}'
            | '\r'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// `String.prototype.trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `String.prototype.trimEnd`.
pub fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// `s.trim().split(/\s+/)`: one empty string for an empty input, as JS gives.
pub fn split_ws(s: &str) -> Vec<&str> {
    let t = trim(s);
    if t.is_empty() {
        return vec![""];
    }
    t.split(is_space).filter(|p| !p.is_empty()).collect()
}

/// The non-empty runs between JS whitespace, with no empty-input sentinel.
pub fn split_spaces(s: &str) -> impl Iterator<Item = &str> {
    s.split(is_space).filter(|t| !t.is_empty())
}

/// `text.trim().replace(/\s+/g, " ")`.
pub fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in trim(s).chars() {
        if is_space(c) {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            in_run = false;
            out.push(c);
        }
    }
    out
}

/// JS `\s` as the body of a regex character class.
pub const SPACE: &str =
    r"\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}";

/// JS `.` (no `s` flag) as a regex atom: everything but the four line terminators.
pub const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

/// `s.length`.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.slice(0, n)` counted in UTF-16 units. A cut that would split a
/// surrogate pair drops the pair instead of leaving half of it.
pub fn slice_utf16(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// `s.slice(0, units)`. A cut through a surrogate pair leaves a replacement
/// character where JavaScript would leave a lone surrogate.
pub fn utf16_prefix(s: &str, units: usize) -> String {
    let prefix: Vec<u16> = s.encode_utf16().take(units).collect();
    String::from_utf16_lossy(&prefix)
}

/// `Math.round`: halves go toward positive infinity.
pub fn round(x: f64) -> f64 {
    let r = x.floor();
    if x - r >= 0.5 { r + 1.0 } else { r }
}

/// `String(n)` for a number.
pub fn num_str(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if n == 0.0 {
        return "0".into();
    }
    let abs = n.abs();
    if !(1e-6..1e21).contains(&abs) {
        let s = format!("{n:e}");
        return match s.split_once('e') {
            Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
            _ => s,
        };
    }
    format!("{n}")
}

/// A number as `JSON.stringify` prints it: a whole value below 9e15 carries
/// no fraction, NaN and the infinities read as null.
pub fn json_number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9e15 { Value::from(n as i64) } else { Value::from(n) }
}

/// `Number.prototype.toFixed`: an exact tie rounds up in magnitude, where
/// Rust's formatter rounds it to even. The sign follows `x < 0`, so a small
/// negative reads `-0.00` and negative zero reads `0.00`.
pub fn to_fixed(x: f64, digits: usize) -> String {
    if !x.is_finite() || x.abs() >= 1e21 {
        return num_str(x);
    }
    let wide = format!("{:.*}", digits + 40, x.abs());
    let (int_part, frac) = wide.split_once('.').unwrap_or((&wide, ""));
    let tail = &frac[digits.min(frac.len())..];
    let tie = tail.starts_with('5') && tail[1..].bytes().all(|b| b == b'0');
    let out = if tie {
        let kept = format!("{}{}", int_part, &frac[..digits]);
        let mut bytes: Vec<u8> = kept.into_bytes();
        let mut i = bytes.len();
        loop {
            if i == 0 {
                bytes.insert(0, b'1');
                break;
            }
            i -= 1;
            if bytes[i] == b'9' {
                bytes[i] = b'0';
            } else {
                bytes[i] += 1;
                break;
            }
        }
        let s = String::from_utf8(bytes).unwrap_or_default();
        if digits == 0 {
            s
        } else {
            let split = s.len() - digits;
            format!("{}.{}", &s[..split], &s[split..])
        }
    } else {
        format!("{:.*}", digits, x.abs())
    };
    if x < 0.0 { format!("-{out}") } else { out }
}

/// `Number(string)`.
pub fn parse_number(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let lower = t.to_ascii_lowercase();
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = lower.strip_prefix(prefix) {
            if !digits.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return f64::NAN;
            }
            return u64::from_str_radix(digits, radix).map_or(f64::NAN, |n| n as f64);
        }
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // Rust's parser also takes "inf" and "nan"; JS does not.
    if !t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// `Number(s) || 0`.
pub fn number_or_zero(s: &str) -> f64 {
    let n = parse_number(s);
    if n.is_nan() { 0.0 } else { n }
}

/// `parseInt(s, 10)`: optional whitespace and sign, then the leading digits;
/// NaN when there are none.
pub fn parse_int(s: &str) -> f64 {
    let t = s.trim_start_matches(is_space);
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits: &str = &rest[..rest.bytes().take_while(u8::is_ascii_digit).count()];
    if digits.is_empty() {
        return f64::NAN;
    }
    let n: f64 = digits.parse().unwrap_or(f64::NAN);
    if neg { -n } else { n }
}

/// `parseInt(s, 10)` for a string of ASCII digits, saturating instead of
/// losing precision past u64.
pub fn parse_digits(s: &str) -> u64 {
    s.parse::<u64>().unwrap_or(u64::MAX)
}

// encodeURIComponent leaves A-Z a-z 0-9 - _ . ! ~ * ' ( ) alone.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// `encodeURIComponent`.
pub fn encode_uri_component(s: &str) -> String {
    utf8_percent_encode(s, URI_COMPONENT).to_string()
}

/// `String(v)` for a defined JSON value.
pub fn to_str(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => num_str(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|e| if e.is_null() { String::new() } else { to_str(e) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// The `_str` helper the JS models share: undefined and null read as "".
pub fn opt_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(v) => to_str(v),
    }
}

/// `String(v)` where the value may be absent, which JS prints as `undefined`.
pub fn str_or_undefined(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".into(), to_str)
}

/// `Number(v)`; an absent field is `undefined`, so NaN.
pub fn to_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => parse_number(s),
        Some(v @ Value::Array(_)) => parse_number(&to_str(v)),
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// `_int`: a rounded finite number, else 0.
pub fn int(v: Option<&Value>) -> i64 {
    let n = to_number(v);
    if n.is_finite() { round(n) as i64 } else { 0 }
}

/// `typeof v === "number" && isFinite(v)`.
pub fn finite_number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()),
        _ => None,
    }
}

/// JS truthiness of an optional JSON value (`undefined` is `None`).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `String(value || "")` over a JSON value.
pub fn str_or_empty(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::Number(n)) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f == 0.0 || f.is_nan() { String::new() } else { num_str(f) }
        }
        Some(Value::Array(items)) => items.iter().map(|e| str_or_empty(Some(e))).collect::<Vec<_>>().join(","),
        Some(other) => to_str(other),
    }
}

/// `typeof value === "string" ? value : ""`.
pub fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

/// `JSON.stringify(value)`.
pub fn stringify(v: &Value) -> String {
    v.to_string()
}

/// `obj[key]` where `obj` may be anything: only objects have fields.
pub fn field<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_object().and_then(|o| o.get(key))
}

/// `typeof v === "object" && v` (arrays included, null excluded).
pub fn is_object_like(v: &Value) -> bool {
    matches!(v, Value::Object(_) | Value::Array(_))
}

/// `for (var k in v)` over a parsed JSON value: integer-like keys first in
/// ascending order, then the rest in insertion order. Array elements are
/// keyed by index.
pub fn entries(v: &Value) -> Vec<(String, &Value)> {
    match v {
        Value::Object(m) => {
            let is_index = |k: &str| {
                k == "0" || (!k.starts_with('0') && !k.is_empty() && k.len() < 10 && k.bytes().all(|b| b.is_ascii_digit()))
            };
            let mut ints: Vec<(u32, &String, &Value)> = Vec::new();
            let mut rest: Vec<(String, &Value)> = Vec::new();
            for (k, val) in m {
                if is_index(k) {
                    ints.push((k.parse().unwrap_or(0), k, val));
                } else {
                    rest.push((k.clone(), val));
                }
            }
            ints.sort_by_key(|(n, _, _)| *n);
            ints.into_iter().map(|(_, k, val)| (k.clone(), val)).chain(rest).collect()
        }
        Value::Array(a) => a.iter().enumerate().map(|(i, e)| (i.to_string(), e)).collect(),
        _ => Vec::new(),
    }
}

static COLLATOR: LazyLock<CollatorBorrowed<'static>> = LazyLock::new(|| {
    Collator::try_new(Default::default(), CollatorOptions::default()).expect("root collation data is compiled in")
});

/// `a.localeCompare(b)` under the root locale.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    COLLATOR.compare(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn whitespace_is_the_js_set() {
        assert_eq!(trim("\u{feff} a \u{a0}"), "a");
        assert_eq!(trim("\u{85}a\u{85}"), "\u{85}a\u{85}");
        assert_eq!(trim_end(" a  "), " a");
        assert_eq!(split_ws("  a  b "), ["a", "b"]);
        assert_eq!(split_ws("  "), [""]);
        assert_eq!(split_spaces(" a\tb ").collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(collapse_spaces("  a \n\t b  "), "a b");
    }

    #[test]
    fn utf16_helpers_count_units() {
        assert_eq!(utf16_len("a\u{1F600}"), 3);
        assert_eq!(slice_utf16("a\u{1F600}b", 2), "a");
        assert_eq!(slice_utf16("a\u{1F600}b", 3), "a\u{1F600}");
        assert_eq!(utf16_prefix("a\u{1F600}", 2), "a\u{FFFD}");
        assert_eq!(utf16_prefix("abc", 2), "ab");
    }

    #[test]
    fn round_goes_half_up() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(-0.4), 0.0);
        assert_eq!(round(42.6), 43.0);
        assert_eq!(round(0.49999999999999994), 0.0);
    }

    #[test]
    fn number_strings_match_js() {
        assert_eq!(num_str(85.0), "85");
        assert_eq!(num_str(-0.0), "0");
        assert_eq!(num_str(0.6), "0.6");
        assert_eq!(num_str(52.52), "52.52");
        assert_eq!(num_str(1e21), "1e+21");
        assert_eq!(num_str(1.5e-7), "1.5e-7");
        assert_eq!(num_str(f64::NAN), "NaN");
        assert_eq!(num_str(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn to_fixed_rounds_ties_up_like_js() {
        assert_eq!(to_fixed(0.25, 1), "0.3");
        assert_eq!(to_fixed(1.45, 1), "1.4");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(9.95, 1), "9.9");
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(9.5, 0), "10");
        assert_eq!(to_fixed(3.6, 1), "3.6");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(29.118083, 2), "29.12");
        assert_eq!(to_fixed(-13.566583, 2), "-13.57");
        assert_eq!(to_fixed(-0.001, 2), "-0.00");
        assert_eq!(to_fixed(-0.0, 2), "0.00");
        assert_eq!(to_fixed(1e21, 2), "1e+21");
    }

    #[test]
    fn parse_number_matches_js() {
        assert_eq!(parse_number(" 42 "), 42.0);
        assert_eq!(parse_number("0x10"), 16.0);
        assert_eq!(parse_number("0X10"), 16.0);
        assert_eq!(parse_number("1e3"), 1000.0);
        assert_eq!(parse_number(""), 0.0);
        assert_eq!(parse_number("-Infinity"), f64::NEG_INFINITY);
        assert!(parse_number("loud").is_nan());
        assert!(parse_number("inf").is_nan());
        assert!(parse_number("nan").is_nan());
        assert!(parse_number("0x+10").is_nan());
        assert!(parse_number("4x").is_nan());
        assert_eq!(number_or_zero("x"), 0.0);
    }

    #[test]
    fn parse_int_reads_a_numeric_prefix() {
        assert_eq!(parse_int("5260 MHz"), 5260.0);
        assert_eq!(parse_int("  -7x"), -7.0);
        assert_eq!(parse_int("\u{feff}+3"), 3.0);
        assert!(parse_int("x5").is_nan());
        assert_eq!(parse_digits("12"), 12);
        assert_eq!(parse_digits("99999999999999999999999"), u64::MAX);
    }


    #[test]
    fn uri_component_matches_js() {
        assert_eq!(encode_uri_component("Tyler, The Creator"), "Tyler%2C%20The%20Creator");
        assert_eq!(encode_uri_component("a-b_c.d!e~f*g'h(i)"), "a-b_c.d!e~f*g'h(i)");
        assert_eq!(encode_uri_component("\u{e9}"), "%C3%A9");
    }

    #[test]
    fn json_strings_read_like_js() {
        assert_eq!(to_str(&json!(2)), "2");
        assert_eq!(to_str(&json!(2.0)), "2");
        assert_eq!(to_str(&json!(2.5)), "2.5");
        assert_eq!(to_str(&json!(null)), "null");
        assert_eq!(to_str(&json!([1, null, "a"])), "1,,a");
        assert_eq!(to_str(&json!({})), "[object Object]");
        assert_eq!(opt_str(None), "");
        assert_eq!(opt_str(Some(&json!(null))), "");
        assert_eq!(str_or_undefined(None), "undefined");
        assert_eq!(str_or_empty(Some(&json!(0))), "");
        assert_eq!(str_or_empty(Some(&json!(true))), "true");
        assert_eq!(str_or_empty(Some(&json!([0, "a"]))), ",a");
        assert_eq!(text(Some(&json!(3))), "");
        assert_eq!(text(Some(&json!("x"))), "x");
    }

    #[test]
    fn json_number_drops_the_fraction_below_9e15() {
        assert_eq!(json_number(3.0).to_string(), "3");
        assert_eq!(json_number(1.5).to_string(), "1.5");
        assert_eq!(json_number(f64::NAN), Value::Null);
        assert_eq!(json_number(5e15).to_string(), "5000000000000000");
    }

    #[test]
    fn json_numbers_read_like_js() {
        assert!(to_number(None).is_nan());
        assert_eq!(to_number(Some(&json!(null))), 0.0);
        assert_eq!(to_number(Some(&json!("12"))), 12.0);
        assert_eq!(to_number(Some(&json!(true))), 1.0);
        assert_eq!(to_number(Some(&json!([]))), 0.0);
        assert_eq!(to_number(Some(&json!([5]))), 5.0);
        assert_eq!(to_number(Some(&json!([null]))), 0.0);
        assert!(to_number(Some(&json!([1, 2]))).is_nan());
        assert!(to_number(Some(&json!({}))).is_nan());
        assert_eq!(int(Some(&json!("2.5"))), 3);
        assert_eq!(int(Some(&json!("x"))), 0);
        assert_eq!(finite_number(Some(&json!(1.5))), Some(1.5));
        assert_eq!(finite_number(Some(&json!("1"))), None);
        assert!(truthy(Some(&json!("a"))));
        assert!(!truthy(Some(&json!(0))));
        assert!(truthy(Some(&json!([]))));
        assert!(!truthy(None));
    }

    #[test]
    fn objects_enumerate_like_for_in() {
        let v: Value = serde_json::from_str(r#"{"b":1,"2":2,"a":3,"1":4}"#).unwrap();
        let keys: Vec<String> = entries(&v).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["1", "2", "b", "a"]);
        assert_eq!(field(&v, "b"), Some(&json!(1)));
        assert_eq!(field(&json!([1]), "0"), None);
        assert!(is_object_like(&json!([])));
        assert!(!is_object_like(&json!(null)));
        assert_eq!(stringify(&serde_json::from_str::<Value>(r#"{"z":1,"a":2}"#).unwrap()), r#"{"z":1,"a":2}"#);
    }

    #[test]
    fn locale_compare_orders_words() {
        assert_eq!(locale_compare("Alpha", "Mid"), Ordering::Less);
        assert_eq!(locale_compare("zeta", "Alpha"), Ordering::Greater);
    }
}
