//! Tool schemas that every client can read.
//!
//! A schema as `schemars` writes it names its types in `$defs` and says an optional field is
//! `null` or something else. Clients do not keep that: the Claude chat shows a `$ref` that leads
//! nowhere and Claude Code an empty `items`, and both lose the type of an optional field. So what a
//! server lists is written out in full: each `$ref` replaced by what it names, and an optional field
//! with its own type (it is optional because it is not in `required`; a `null` is still accepted).

use rmcp::model::JsonObject;
use serde_json::{Map, Value};

/// `schema` with no `$defs`, no `$ref` and no `null` alternative.
pub(crate) fn stand_alone(schema: &JsonObject) -> JsonObject {
    let mut schema = schema.clone();
    let defs = match schema.remove("$defs") {
        Some(Value::Object(defs)) => defs,
        _ => Map::new(),
    };
    for value in schema.values_mut() {
        write_out(value, &defs);
    }
    schema
}

fn write_out(value: &mut Value, defs: &Map<String, Value>) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(|item| write_out(item, defs)),
        Value::Object(object) => {
            drop_null_alternative(object);
            replace_reference(object, defs);
            drop_null_type(object);
            object.values_mut().for_each(|child| write_out(child, defs));
        }
        _ => {}
    }
}

/// `anyOf: [X, null]` becomes X, under what is already said here (the field's description).
fn drop_null_alternative(object: &mut Map<String, Value>) {
    let Some(Value::Array(alternatives)) = object.get_mut("anyOf") else {
        return;
    };
    alternatives.retain(|alternative| alternative.get("type").is_none_or(|t| t != "null"));
    if let [Value::Object(only)] = alternatives.as_slice() {
        let only = only.clone();
        object.remove("anyOf");
        add_missing(object, only);
    }
}

/// `$ref` becomes what it names, under what is already said here.
fn replace_reference(object: &mut Map<String, Value>, defs: &Map<String, Value>) {
    let named = object
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .and_then(|name| defs.get(name));
    if let Some(Value::Object(named)) = named {
        let named = named.clone();
        object.remove("$ref");
        add_missing(object, named);
    }
}

/// `type: [T, "null"]` becomes `type: T`.
fn drop_null_type(object: &mut Map<String, Value>) {
    if let Some(Value::Array(types)) = object.get_mut("type") {
        types.retain(|name| name != "null");
        if let [only] = types.as_slice() {
            let only = only.clone();
            object.insert("type".to_owned(), only);
        }
    }
}

fn add_missing(object: &mut Map<String, Value>, from: Map<String, Value>) {
    for (key, value) in from {
        object.entry(key).or_insert(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn written_out(schema: Value) -> Value {
        let Value::Object(schema) = schema else {
            panic!("not an object");
        };
        Value::Object(stand_alone(&schema))
    }

    #[test]
    fn a_reference_is_replaced_and_the_field_keeps_its_own_description() {
        let schema = written_out(json!({
            "type": "object",
            "properties": {
                "region": {
                    "anyOf": [{ "$ref": "#/$defs/Region" }, { "type": "null" }],
                    "description": "Part of the page."
                },
                "kinds": { "type": "array", "items": { "$ref": "#/$defs/Kind" } }
            },
            "$defs": {
                "Region": {
                    "type": "object",
                    "description": "A rectangle.",
                    "properties": { "kind": { "$ref": "#/$defs/Kind" } }
                },
                "Kind": { "type": "string", "enum": ["a", "b"] }
            }
        }));
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {
                    "region": {
                        "type": "object",
                        "description": "Part of the page.",
                        "properties": { "kind": { "type": "string", "enum": ["a", "b"] } }
                    },
                    "kinds": { "type": "array", "items": { "type": "string", "enum": ["a", "b"] } }
                }
            })
        );
    }

    #[test]
    fn an_optional_field_keeps_its_type() {
        let schema = written_out(json!({
            "properties": {
                "dpi": { "type": ["integer", "null"], "minimum": 25 },
                "ids": { "type": ["array", "null"], "items": { "type": "integer" } },
                "either": { "anyOf": [{ "type": "string" }, { "type": "number" }] },
                // A field named like a keyword is not a keyword.
                "type": { "type": "string" }
            }
        }));
        assert_eq!(schema["properties"]["dpi"]["type"], "integer");
        assert_eq!(schema["properties"]["ids"]["type"], "array");
        assert_eq!(schema["properties"]["type"]["type"], "string");
        assert_eq!(
            schema["properties"]["either"]["anyOf"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
}
