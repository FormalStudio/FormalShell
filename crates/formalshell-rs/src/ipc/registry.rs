//! The targets and how a request against them is answered, worded as
//! the golden contract in `tests/ipc-golden.jsonl` words it: every error lands
//! on stdout and the client still exits 0.

use super::wire::Request;

pub use serde_json::Value as Json;

/// What a function answers: a value to print, or the reason it refused.
/// `ok()` is the bare acknowledgement, `text(..)` a string printed as it is,
/// and any other JSON prints compact; the registry alone words `ok`,
/// `error: ...` and numbers.
pub type Reply = Result<Json, String>;

pub fn ok() -> Reply {
    Ok(Json::Null)
}

pub fn text(s: impl Into<String>) -> Reply {
    Ok(Json::String(s.into()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Void,
    String,
    Int,
    Bool,
    Real,
}

impl Type {
    fn name(self) -> &'static str {
        match self {
            Type::Void => "void",
            Type::String => "string",
            Type::Int => "int",
            Type::Bool => "bool",
            Type::Real => "real",
        }
    }

    /// `IpcValueSlot::setString`: Qt's `toInt` and `toDouble` trim
    /// whitespace and take a sign, and a bool is `true`, `false` or any int.
    fn parse(self, text: &str) -> Option<Value> {
        match self {
            Type::Void => None,
            Type::String => Some(Value::Str(text.to_owned())),
            Type::Int => text.trim().parse().ok().map(Value::Int),
            Type::Bool => match text {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                _ => text.trim().parse::<i32>().ok().map(|n| Value::Bool(n != 0)),
            },
            Type::Real => text.trim().parse().ok().map(Value::Real),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Int(i32),
    Bool(bool),
    Real(f64),
}

impl Value {
    pub fn str(&self) -> &str {
        match self {
            Value::Str(s) => s,
            _ => "",
        }
    }

    pub fn int(&self) -> i32 {
        match self {
            Value::Int(n) => *n,
            _ => 0,
        }
    }

    pub fn bool(&self) -> bool {
        matches!(self, Value::Bool(true))
    }

    pub fn real(&self) -> f64 {
        match self {
            Value::Real(r) => *r,
            _ => 0.0,
        }
    }
}

pub struct Function<C> {
    pub name: &'static str,
    pub params: &'static [(&'static str, Type)],
    pub ret: Type,
    pub call: fn(&mut C, &[Value]) -> Reply,
}

impl<C> Function<C> {
    pub fn definition(&self) -> String {
        let params: Vec<String> = self.params.iter().map(|(name, ty)| format!("{name}: {}", ty.name())).collect();
        format!("function {}({}): {}", self.name, params.join(", "), self.ret.name())
    }
}

pub struct Target<C> {
    pub name: &'static str,
    pub functions: Vec<Function<C>>,
}

impl<C> Target<C> {
    fn definition(&self) -> String {
        let mut out = format!("target {}", self.name);
        for function in &self.functions {
            out.push_str("\n  ");
            out.push_str(&function.definition());
        }
        out
    }

    fn function(&self, name: &str) -> Option<&Function<C>> {
        self.functions.iter().find(|f| f.name == name)
    }
}

pub struct Registry<C> {
    pub targets: Vec<Target<C>>,
}

impl<C> Registry<C> {
    fn target(&self, name: &str) -> Option<&Target<C>> {
        self.targets.iter().find(|t| t.name == name)
    }

    /// The text the client prints on stdout.
    pub fn answer(&self, ctx: &mut C, request: &Request) -> String {
        match request {
            Request::Call { target, function, args } => self.call(ctx, target, function, args),
            Request::Show { target, name } => self.show(target, name),
            // No target has a signal or a property yet.
            Request::Signal { target, .. } => match self.target(target) {
                Some(_) => "Signal not found.\n".into(),
                None => "Target not found.\n".into(),
            },
            Request::Prop { target, .. } => match self.target(target) {
                Some(_) => "Property not found.\n".into(),
                None => "Target not found.\n".into(),
            },
        }
    }

    fn call(&self, ctx: &mut C, target: &str, function: &str, args: &[String]) -> String {
        let Some(target) = self.target(target) else { return "Target not found.\n".into() };
        let Some(function) = target.function(function) else { return "Function not found.\n".into() };
        let required = function.params.len();
        if required != args.len() {
            let which = if required < args.len() { "many" } else { "few" };
            return format!(
                "Too {which} arguments provided ({required} required but {} were provided.)\nFunction definition: {}\n",
                args.len(),
                function.definition()
            );
        }
        let mut values = Vec::with_capacity(required);
        for (i, ((_, ty), arg)) in function.params.iter().zip(args).enumerate() {
            match ty.parse(arg) {
                Some(value) => values.push(value),
                None => {
                    return format!(
                        "Unable to parse argument {} as {}. Provided argument: {}\nFunction definition: {}\n",
                        i + 1,
                        qdebug_quote(ty.name()),
                        qdebug_quote(arg),
                        function.definition()
                    );
                }
            }
        }
        if function.ret == Type::Void {
            return String::new();
        }
        match (function.call)(ctx, &values) {
            Ok(json) => format!("{}\n", print(function.ret, &json)),
            Err(reason) => format!("error: {reason}\n"),
        }
    }

    fn show(&self, target: &str, name: &str) -> String {
        if target.is_empty() {
            return self.targets.iter().map(|t| t.definition() + "\n").collect();
        }
        let Some(target) = self.target(target) else { return "Target not found.\n".into() };
        if name.is_empty() {
            return target.definition() + "\n";
        }
        match target.function(name) {
            Some(function) => function.definition() + "\n",
            None => "Function not found.\n".into(),
        }
    }
}

/// `IpcValueSlot::toString`: a string as it is, `null` as `ok`, a real the
/// way Qt prints one, and any other JSON compact.
fn print(ret: Type, json: &Json) -> String {
    match json {
        Json::Null => "ok".into(),
        Json::String(s) => s.clone(),
        Json::Number(n) if ret == Type::Real => qt_number(n.as_f64().unwrap_or(0.0)),
        other => other.to_string(),
    }
}

/// How `QDebug` prints a quoted `QString`.
fn qdebug_quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `QString::number(double)`: printf's `%g` at six significant digits.
fn qt_number(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{value:.5e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s.to_owned() }
    };
    if !(-4..6).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", trim(mantissa), exp.abs())
    } else {
        trim(&format!("{value:.*}", (5 - exp) as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_prints_as_qt_does() {
        for (value, text) in [
            (0.1, "0.1"),
            (3.0, "3"),
            (1e21, "1e+21"),
            (1e-7, "1e-07"),
            (123456789.0, "1.23457e+08"),
            (1.5, "1.5"),
            (-2.25, "-2.25"),
            (999999.0, "999999"),
            (9999999.0, "1e+07"),
            (0.0001, "0.0001"),
        ] {
            assert_eq!(qt_number(value), text, "{value}");
        }
    }
}
