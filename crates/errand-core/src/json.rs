//! Lenient reads of JSON other apps write: a field of the wrong type counts as absent.

use serde_json::{Map, Value};

/// A string with at least one character.
pub fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|text| !text.is_empty())
}

/// A JSON number. Booleans are not numbers here.
pub fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

pub fn object(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

/// A flag another app wrote: true, or a non-zero number.
pub fn flag(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        _ => false,
    }
}

/// Length in UTF-16 code units, the unit the apps' own limits were written in.
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}
