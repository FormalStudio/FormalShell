//! A desktop entry's `Exec` line: the one place its field codes are read.

use fs_js as js;

/// Field codes per the freedesktop spec. `%%` is a literal percent and is
/// not one of them.
const FIELD_CODES: &str = "fFuUickdDnNvm";

/// `exec` with its field codes removed and `%%` made one `%`. One pass, so a
/// literal `%%` can never be re-read as the start of a field code.
fn drop_codes(exec: &str) -> String {
    let mut out = String::new();
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        // `.` in the JS pattern skips line terminators.
        match chars.peek().copied() {
            Some(n) if !matches!(n, '\n' | '\r' | '\u{2028}' | '\u{2029}') => {
                chars.next();
                if n == '%' {
                    out.push('%');
                } else if !FIELD_CODES.contains(n) {
                    out.push('%');
                    out.push(n);
                }
            }
            _ => out.push('%'),
        }
    }
    out
}

/// [`drop_codes`] with the whitespace collapsed, for a line handed to
/// `sh -c` whole.
pub fn strip_field_codes(exec: &str) -> String {
    js::split_ws(&drop_codes(exec)).into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The line as an argv: quotes and backslashes honoured, field codes dropped.
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut live = false;
    let mut quote: Option<char> = None;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                live = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    live = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if live || !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                    live = false;
                }
            }
            (None, c) => {
                cur.push(c);
                live = true;
            }
        }
    }
    if live || !cur.is_empty() {
        args.push(cur);
    }
    args.iter().map(|a| drop_codes(a)).filter(|a| !a.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_field_codes_removes_a_single_code() {
        assert_eq!(strip_field_codes("firefox %u"), "firefox");
    }

    #[test]
    fn strip_field_codes_collapses_multiple_codes_cleanly() {
        assert_eq!(strip_field_codes("foo %F --bar %i"), "foo --bar");
    }

    #[test]
    fn strip_field_codes_keeps_a_literal_percent_from_double_percent() {
        assert_eq!(strip_field_codes("echo 100%%"), "echo 100%");
        assert_eq!(strip_field_codes("a %f 100%% b"), "a 100% b");
    }

    #[test]
    fn split_exec_honours_quotes_and_drops_codes() {
        assert_eq!(split_exec("foot --app-id=x %U"), ["foot", "--app-id=x"]);
        assert_eq!(split_exec(r#"env "A B"=1 /usr/bin/app %%x"#), ["env", "A B=1", "/usr/bin/app", "%x"]);
        assert!(split_exec("").is_empty());
    }
}
