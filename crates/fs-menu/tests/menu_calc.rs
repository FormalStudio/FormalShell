use fs_menu::calc::{evaluate, format, result_node};
use fs_menu::node::Kind;

#[test]
fn precedence() {
    assert_eq!(evaluate("2+2*3"), Some(8.0));
    assert_eq!(evaluate("2*3+2"), Some(8.0));
    assert_eq!(evaluate("10-4/2"), Some(8.0));
    assert_eq!(evaluate("10%3"), Some(1.0));
    assert_eq!(evaluate("7%4*2"), Some(6.0));
}

#[test]
fn parentheses() {
    assert_eq!(evaluate("(2+2)*3"), Some(12.0));
    assert_eq!(evaluate("2*(3+(4-1))"), Some(12.0));
}

#[test]
fn unary_minus() {
    assert_eq!(evaluate("-5+3"), Some(-2.0));
    assert_eq!(evaluate("2*-3"), Some(-6.0));
    assert_eq!(evaluate("-(2+3)"), Some(-5.0));
    // ^ binds tighter than unary minus (math convention).
    assert_eq!(evaluate("-2^2"), Some(-4.0));
}

#[test]
fn power() {
    assert_eq!(evaluate("2^10"), Some(1024.0));
    // Right-associative: 2^(3^2), not (2^3)^2 = 64.
    assert_eq!(evaluate("2^3^2"), Some(512.0));
    assert_eq!(evaluate("2^-1"), Some(0.5));
}

#[test]
fn decimals_and_whitespace() {
    assert_eq!(evaluate("1.5*2"), Some(3.0));
    assert_eq!(evaluate(".5+.5"), Some(1.0));
    assert_eq!(evaluate(" 2 + 2 * 3 "), Some(8.0));
}

#[test]
fn invalid_returns_none() {
    for text in ["", "firefox", "2+", "(2", "2 2", "1.2.3", ".", "2$3", "1_000+1"] {
        assert_eq!(evaluate(text), None, "{text:?}");
    }
}

#[test]
fn nonfinite_returns_none() {
    assert_eq!(evaluate("1/0"), None);
    assert_eq!(evaluate("0/0"), None);
    assert_eq!(evaluate("5%0"), None);
}

#[test]
fn format_trims_float_noise() {
    assert_eq!(format(0.1 + 0.2), "0.3");
    assert_eq!(format(8.0), "8");
    assert_eq!(format(-2.5), "-2.5");
}

// ECMAScript's toPrecision rounds a tie up and Number::toString switches to
// exponent form at 1e21 and 1e-7; both reach the wl-copy argument.
#[test]
fn format_follows_ecmascript_number_rules() {
    assert_eq!(format(1_000_000_000_005.0), "1000000000010");
    assert_eq!(format(1e21), "1e+21");
    assert_eq!(format(1.5e-7), "1.5e-7");
    assert_eq!(format(0.000001), "0.000001");
    assert_eq!(format(1.0 / 3.0), "0.333333333333");
    assert_eq!(format(123_456_789.123_456_79), "123456789.123");
}

#[test]
fn result_node_shape() {
    let node = result_node("2+2*3").expect("parses");
    assert_eq!(node.label, "= 8");
    assert_eq!(node.kind, Kind::Action);
    assert_eq!(node.meta.as_deref(), Some("CALC"));
    assert_eq!(node.action.as_ref().map(ToString::to_string).as_deref(), Some("wl-copy -- 8"));
    assert!(result_node("not math").is_none());
}
