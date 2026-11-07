//! What an assistant is told of the orders of `preview_revision`, `create_revision` and
//! `edit_revision`: all of it comes from the doc comments of [`ardoise_revise::Order`].
//!
//! Not in the schemas of those tools: written out, the orders would weigh tens of kilobytes in each
//! of the three, read at every connection (see `crate::schema` for why they must be written out).
//! So a tool's schema only names the `op`s ([`order_list`]), `revision_orders` answers the whole
//! schema on demand ([`revision_orders`]), and a badly written order is told its fields
//! ([`orders_one_by_one`]).

use ardoise_revise::Order;
use schemars::{Schema, SchemaGenerator, json_schema, schema_for};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::{ToolResult, json_result};

/// The JSON schema of one order, with its doc comments.
fn order_schema() -> Value {
    serde_json::to_value(schema_for!(Order)).unwrap_or_default()
}

fn variants(schema: &Value) -> &[Value] {
    schema["oneOf"].as_array().map_or(&[], Vec::as_slice)
}

fn op_of(variant: &Value) -> Option<&str> {
    variant["properties"]["op"]["const"].as_str()
}

/// The `orders` of a tool's schema: a list of objects, each with one of the `op`s.
pub(super) fn order_list(_: &mut SchemaGenerator) -> Schema {
    let schema = order_schema();
    let ops: Vec<&str> = variants(&schema).iter().filter_map(op_of).collect();
    json_schema!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": { "op": { "type": "string", "enum": ops } },
            "required": ["op"]
        }
    })
}

pub fn revision_orders() -> ToolResult {
    json_result(json!({
        "about": "The JSON schema of one item of `orders` in `preview_revision`, `create_revision` and `edit_revision`: `oneOf` has one entry per `op` with what it does and what each of its fields means, and `$ref` names a type in `$defs`. Everything is in millimetres from the top left of the page, y pointing down.",
        "order": order_schema(),
    }))
}

/// Reads a list of orders one by one, so that a badly written one is named as the engine names
/// its own, and told all its fields at once: in a long list, "missing field `color`" alone says
/// nothing an assistant can act on, and it would find the next field missing at the next try.
pub(super) fn orders_one_by_one<'de, D>(deserializer: D) -> Result<Vec<Order>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<Value>::deserialize(deserializer)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let op = value
                .get("op")
                .and_then(Value::as_str)
                .unwrap_or("no op")
                .to_owned();
            serde_json::from_value(value).map_err(|error| {
                let fields = fields_of(&op)
                    .map(|fields| format!(" {fields}."))
                    .unwrap_or_default();
                serde::de::Error::custom(format!(
                    "order {index} ({op}): {error}.{fields} `revision_orders` gives every order and what its fields mean"
                ))
            })
        })
        .collect()
}

/// The fields of the order `op` and their types, in a sentence: `None` for an unknown `op`, for
/// which the error already lists the others.
fn fields_of(op: &str) -> Option<String> {
    let schema = order_schema();
    let variant = variants(&schema)
        .iter()
        .find(|variant| op_of(variant) == Some(op))?;
    let required = variant["required"].as_array()?;
    let (mut needed, mut optional) = (Vec::new(), Vec::new());
    for (name, field) in variant["properties"].as_object()? {
        if name == "op" {
            continue;
        }
        let described = format!("`{name}` ({})", kind(field, &schema));
        if required.iter().any(|field| field == name.as_str()) {
            needed.push(described);
        } else {
            optional.push(described);
        }
    }
    let mut sentence = format!("A `{op}` order needs ");
    sentence.push_str(&if needed.is_empty() {
        "nothing else".to_owned()
    } else {
        needed.join(", ")
    });
    if !optional.is_empty() {
        sentence.push_str(" and can have ");
        sentence.push_str(&optional.join(", "));
    }
    Some(sentence)
}

/// A field's type in a few words: `number`, `[number, number]`, `list of Shape`, and the fields of
/// a named object, which is where `select` is misunderstood.
fn kind(field: &Value, schema: &Value) -> String {
    if let Some(reference) = field["$ref"].as_str() {
        let name = reference.rsplit('/').next().unwrap_or(reference);
        return match schema["$defs"][name]["properties"].as_object() {
            Some(properties) => format!("{name}: {}", names(properties)),
            None => name.to_owned(),
        };
    }
    if let Some(alternatives) = field["anyOf"].as_array() {
        let named = alternatives
            .iter()
            .find(|alternative| alternative["type"] != "null");
        return named.map_or_else(|| "any".to_owned(), |alternative| kind(alternative, schema));
    }
    let type_name = match &field["type"] {
        Value::String(name) => name.as_str(),
        Value::Array(names) => names
            .iter()
            .filter_map(Value::as_str)
            .find(|name| *name != "null")
            .unwrap_or("any"),
        _ => "any",
    };
    if type_name != "array" {
        return type_name.to_owned();
    }
    let item = kind(&field["items"], schema);
    match (field["minItems"].as_u64(), field["maxItems"].as_u64()) {
        (Some(least), Some(most)) if least == most => {
            format!("[{}]", vec![item; least as usize].join(", "))
        }
        _ => format!("list of {item}"),
    }
}

fn names(properties: &Map<String, Value>) -> String {
    let names: Vec<&str> = properties.keys().map(String::as_str).collect();
    names.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_order_has_its_fields_told() {
        let schema = order_schema();
        let ops: Vec<&str> = variants(&schema).iter().filter_map(op_of).collect();
        assert_eq!(ops.len(), 14, "{ops:?}");
        for op in ops {
            assert!(fields_of(op).is_some(), "{op}");
        }
        assert_eq!(fields_of("no such order"), None);
    }

    #[test]
    fn the_fields_of_an_order_say_their_types() {
        let rotate = fields_of("rotate").unwrap();
        assert!(rotate.contains("`degrees` (number)"), "{rotate}");
        assert!(rotate.contains("`origin` ([number, number])"), "{rotate}");
        // What goes in a selection, the field most often misunderstood.
        assert!(rotate.contains("`select` (Select: "), "{rotate}");
        assert!(rotate.contains("ids"), "{rotate}");

        let glyph = fields_of("glyph").unwrap();
        assert!(glyph.contains("`parts` (list of Part: "), "{glyph}");
        assert!(glyph.contains("and can have `after` (integer)"), "{glyph}");

        let add = fields_of("add").unwrap();
        assert!(add.contains("`tool` (PenTool)"), "{add}");
    }
}
