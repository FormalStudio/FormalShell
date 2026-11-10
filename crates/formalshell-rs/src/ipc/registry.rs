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

/// A real, printed the way Qt prints one. JSON has no NaN or infinity, so
/// those travel as the words Qt prints for them.
#[cfg_attr(not(test), expect(dead_code))]
pub fn real(v: f64) -> Reply {
    if v.is_finite() { Ok(Json::from(v)) } else { text(qt_number(v)) }
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

    /// Ints and reals are trimmed and take a sign, and a bool is `true`,
    /// `false` or any int.
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
    /// Its calls grab the screen, so a launcher row running one waits
    /// for the card to leave it.
    pub reads_screen: bool,
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
    pub fn reads_screen(&self, target: &str) -> bool {
        self.target(target).is_some_and(|t| t.reads_screen)
    }

    fn target(&self, name: &str) -> Option<&Target<C>> {
        self.targets.iter().find(|t| t.name == name)
    }

    /// Why `args` would not reach `target.function`, in the words a log
    /// line reads well in.
    pub fn check(&self, target: &str, function: &str, args: &[String]) -> Result<(), String> {
        self.bind(target, function, args).map(drop)
    }

    /// `target.function` run in process, refused in [`Registry::check`]'s
    /// words.
    pub fn run(&self, ctx: &mut C, target: &str, function: &str, args: &[String]) -> Reply {
        let (f, values) = self.bind(target, function, args)?;
        (f.call)(ctx, &values)
    }

    fn bind(&self, target: &str, function: &str, args: &[String]) -> Result<(&Function<C>, Vec<Value>), String> {
        let Some(t) = self.target(target) else { return Err(format!("no IPC target \"{target}\"")) };
        let Some(f) = t.function(function) else { return Err(format!("target {target} has no function \"{function}\"")) };
        if f.params.len() != args.len() {
            return Err(format!("{} takes {} arguments, given {}", f.definition(), f.params.len(), args.len()));
        }
        let values = f
            .params
            .iter()
            .zip(args)
            .map(|((name, ty), arg)| ty.parse(arg).ok_or_else(|| format!("{target}.{function}: {name} {arg:?} is not {}", ty.name())))
            .collect::<Result<_, _>>()?;
        Ok((f, values))
    }

    /// How many arguments `target.function` takes, if it is one.
    pub fn arity(&self, target: &str, function: &str) -> Option<usize> {
        self.target(target)?.function(function).map(|f| f.params.len())
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
        let reply = (function.call)(ctx, &values);
        if function.ret == Type::Void {
            return String::new();
        }
        match reply {
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

/// A string as it is, `null` as `ok`, a real the
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
