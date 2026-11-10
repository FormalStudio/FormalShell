//! Safe expression calculator behind the CALC row: a recursive-descent parser
//! over `+ - * / % ^`, parentheses, unary minus and plain decimal numbers (no
//! underscore separators), so a search query can never execute anything.
//! `evaluate` returns a finite number or `None`; every keystroke in the
//! search field runs through it.
//!
//! Grammar (`^` right-associative, binding tighter than unary minus, so
//! `-2^2 = -4` and `2^-3` works):
//!
//! ```text
//! expr    := term { ("+" | "-") term }
//! term    := factor { ("*" | "/" | "%") factor }
//! factor  := "-" factor | power
//! power   := primary [ "^" factor ]
//! primary := number | "(" expr ")"
//! ```

use crate::node::{Kind, Node};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Num(f64),
    Op(char),
}

/// Deeper than any real query and shallow enough for the stack.
const MAX_DEPTH: usize = 512;

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == ' ' || c == '\t' {
            i += 1;
            continue;
        }
        if "+-*/%^()".contains(c) {
            tokens.push(Token::Op(c));
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '.' {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let raw: String = chars[start..i].iter().collect();
            if raw == "." {
                return None;
            }
            tokens.push(Token::Num(raw.parse().ok()?));
            continue;
        }
        return None;
    }
    Some(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(Token::Op(op @ ('+' | '-'))) = self.peek() {
            self.pos += 1;
            let rhs = self.term()?;
            v = if op == '+' { v + rhs } else { v - rhs };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.factor()?;
        while let Some(Token::Op(op @ ('*' | '/' | '%'))) = self.peek() {
            self.pos += 1;
            let rhs = self.factor()?;
            v = match op {
                '*' => v * rhs,
                '/' => v / rhs,
                _ => v % rhs,
            };
        }
        Some(v)
    }

    fn factor(&mut self) -> Option<f64> {
        if self.peek() == Some(Token::Op('-')) {
            self.pos += 1;
            return self.nested(Parser::factor).map(|v| -v);
        }
        self.power()
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.primary()?;
        if self.peek() == Some(Token::Op('^')) {
            self.pos += 1;
            let exp = self.nested(Parser::factor)?;
            return Some(base.powf(exp));
        }
        Some(base)
    }

    fn primary(&mut self) -> Option<f64> {
        match self.peek()? {
            Token::Num(n) => {
                self.pos += 1;
                Some(n)
            }
            Token::Op('(') => {
                self.pos += 1;
                let v = self.nested(Parser::expr)?;
                if self.peek() != Some(Token::Op(')')) {
                    return None;
                }
                self.pos += 1;
                Some(v)
            }
            Token::Op(_) => None,
        }
    }

    fn nested(&mut self, rule: fn(&mut Parser) -> Option<f64>) -> Option<f64> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let v = rule(self);
        self.depth -= 1;
        v
    }
}

/// The value of `text`, or `None` when it is not a finite arithmetic
/// expression.
pub fn evaluate(text: &str) -> Option<f64> {
    let tokens = tokenize(text)?;
    if tokens.is_empty() {
        return None;
    }
    let mut parser = Parser { tokens, pos: 0, depth: 0 };
    let v = parser.expr()?;
    if parser.pos != parser.tokens.len() {
        return None;
    }
    v.is_finite().then_some(v)
}

/// `n.toPrecision(12)` as `(negative, digits, exponent)` where the value is
/// `0.d1d2d3... * 10^exponent`. Rounds half up on the exact decimal expansion,
/// which is what ECMAScript specifies (Rust's own formatter rounds half even).
fn to_precision_12(n: f64) -> (bool, Vec<u8>, i32) {
    const SIGNIFICANT: usize = 12;
    // Every f64 has a finite decimal expansion of well under 800 digits.
    let exact = format!("{:.800e}", n.abs());
    let (mantissa, exp) = exact.split_once('e').unwrap_or((&exact, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    let round_up = digits[SIGNIFICANT] >= 5;
    digits.truncate(SIGNIFICANT);
    let mut exponent = exp + 1;
    if round_up {
        let mut k = SIGNIFICANT;
        loop {
            if k == 0 {
                digits.insert(0, 1);
                digits.truncate(SIGNIFICANT);
                exponent += 1;
                break;
            }
            k -= 1;
            if digits[k] == 9 {
                digits[k] = 0;
            } else {
                digits[k] += 1;
                break;
            }
        }
    }
    (n.is_sign_negative(), digits, exponent)
}

/// 12 significant digits then re-parse: collapses float noise (0.1+0.2 ->
/// "0.3") without freezing integers into exponent notation.
pub fn format(n: f64) -> String {
    if !n.is_finite() {
        return if n.is_nan() { "NaN".to_string() } else if n > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() };
    }
    if n == 0.0 {
        return "0".to_string();
    }
    let (negative, digits, exponent) = to_precision_12(n);
    let digit_string: String = digits.iter().map(|d| char::from(b'0' + d)).collect();
    let rounded: f64 = std::format!("{}0.{digit_string}e{exponent}", if negative { "-" } else { "" })
        .parse()
        .unwrap_or(n);
    fs_js::num_str(rounded)
}

/// The id of the node `result_node` makes.
pub const RESULT_ID: &str = "calc.result";

/// The ready-made row the launcher prepends to ranked results (and shows
/// alone at the calc route) when the query parses. A plain action node, so
/// Enter needs no new handling. `--` keeps a leading minus from reading as a
/// wl-copy option.
pub fn result_node(query: &str) -> Option<Node> {
    let value = evaluate(query)?;
    let text = format(value);
    Some(Node {
        parent_id: Some("calc".to_string()),
        action: Some(crate::actions::copy_run(&text)),
        meta: Some("CALC".to_string()),
        verb: Some("Copy".to_string()),
        actions: vec![crate::node::ActionEntry { paste_after: Some(true), ..crate::node::ActionEntry::new("paste", "Paste", crate::actions::copy_run(&text)) }],
        ..Node::new(RESULT_ID, std::format!("= {text}"), Kind::Action)
    })
}
