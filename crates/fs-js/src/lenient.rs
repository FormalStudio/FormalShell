//! Reading a JSON file a hand, an older shell or another program may have
//! written: a value of the wrong type reads as
//! absent, and in a list or map one bad entry costs only itself.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::{DeserializeOwned, Deserializer};
use serde_json::Value;

/// `deserialize_with` for an `Option` field under `#[serde(default)]`.
pub fn field<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(d)?).ok())
}

/// `deserialize_with` for an `Option` map field: entries of the wrong type
/// are dropped.
pub fn entries<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<BTreeMap<String, T>>, D::Error> {
    Ok(map(Value::deserialize(d)?))
}

/// `None` unless `value` is an object; entries of the wrong type are dropped.
pub fn map<T: DeserializeOwned>(value: Value) -> Option<BTreeMap<String, T>> {
    let Value::Object(object) = value else { return None };
    // Inserted one at a time: collecting sorts, and each `T` would carry a
    // stable sort of its own for keys that arrive unique.
    let mut map = BTreeMap::new();
    for (k, v) in object {
        if let Ok(v) = serde_json::from_value(v) {
            map.insert(k, v);
        }
    }
    Some(map)
}

/// `None` unless `value` is an array; items of the wrong type are dropped.
pub fn list<T: DeserializeOwned>(value: Value) -> Option<Vec<T>> {
    let Value::Array(items) = value else { return None };
    Some(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_bad_entry_costs_only_itself() {
        assert_eq!(map::<i64>(json!({"a": 1, "b": "x"})), Some([("a".to_string(), 1)].into()));
        assert_eq!(list::<i64>(json!([1, "x", 2])), Some(vec![1, 2]));
        assert_eq!(map::<i64>(json!([1])), None);
        assert_eq!(list::<i64>(json!({})), None);
    }
}
