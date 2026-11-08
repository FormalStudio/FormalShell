use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

/// An `a{sv}` as zbus reads it.
pub type Props = HashMap<String, OwnedValue>;

/// One entry of an `a{sv}`, read through any variants it is nested in. None
/// when it is missing or of another type.
pub fn prop<T: TryFrom<OwnedValue>>(props: &Props, name: &str) -> Option<T> {
    T::try_from(plain(props.get(name)?).try_to_owned().ok()?).ok()
}

/// The value inside any variants it is nested in: some senders wrap a
/// property in a second `v`.
pub fn plain<'a, 'b>(value: &'a Value<'b>) -> &'a Value<'b> {
    match value {
        Value::Value(inner) => plain(inner),
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{OwnedObjectPath, Structure};

    fn props(entries: Vec<(&str, Value<'static>)>) -> Props {
        entries.into_iter().map(|(k, v)| (k.to_owned(), v.try_to_owned().unwrap())).collect()
    }

    #[test]
    fn reads_a_plain_and_a_nested_variant_alike() {
        let p = props(vec![
            ("Percentage", Value::F64(41.0)),
            ("Model", Value::Value(Box::new(Value::Value(Box::new(Value::from("FAKE")))))),
            ("Present", Value::Value(Box::new(Value::Bool(true)))),
        ]);
        assert_eq!(prop::<f64>(&p, "Percentage"), Some(41.0));
        assert_eq!(prop::<String>(&p, "Model").as_deref(), Some("FAKE"));
        assert_eq!(prop::<bool>(&p, "Present"), Some(true));
    }

    #[test]
    fn a_missing_or_mistyped_entry_is_none() {
        let p = props(vec![("Type", Value::U32(2))]);
        assert_eq!(prop::<u32>(&p, "State"), None);
        assert_eq!(prop::<String>(&p, "Type"), None);
        assert_eq!(prop::<i32>(&p, "Type"), None);
    }

    #[test]
    fn reads_paths_and_structures() {
        let path = OwnedObjectPath::try_from("/org/example/Menu").unwrap();
        let tip = Structure::from(("icon", "title"));
        let p = props(vec![
            ("Menu", Value::Value(Box::new(Value::from(path.clone())))),
            ("ToolTip", Value::from(tip)),
        ]);
        assert_eq!(prop::<OwnedObjectPath>(&p, "Menu"), Some(path));
        let tip = prop::<Structure<'static>>(&p, "ToolTip").unwrap();
        assert_eq!(<(String, String)>::try_from(tip).unwrap(), ("icon".into(), "title".into()));
    }
}
