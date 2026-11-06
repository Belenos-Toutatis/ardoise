//! The JSON schema an assistant reads: every order is there, and every field is explained.

use serde_json::Value;

use super::*;

fn schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Vec<Order>)).unwrap()
}

/// Calls `visit` on every object of the schema that has `properties`.
fn each_object(value: &Value, visit: &mut impl FnMut(&Value)) {
    match value {
        Value::Object(map) => {
            if map.contains_key("properties") {
                visit(value);
            }
            for inner in map.values() {
                each_object(inner, visit);
            }
        }
        Value::Array(items) => {
            for inner in items {
                each_object(inner, visit);
            }
        }
        _ => {}
    }
}

#[test]
fn every_order_is_in_the_schema() {
    let schema = schema();
    let mut ops: Vec<String> = Vec::new();
    each_object(&schema, &mut |object| {
        if let Some(op) = object["properties"].get("op") {
            ops.push(op["const"].as_str().unwrap_or_default().to_owned());
        }
    });
    ops.sort();
    assert_eq!(
        ops,
        [
            "add",
            "copy",
            "delete",
            "glyph",
            "move",
            "recolor",
            "replace",
            "restyle",
            "retime",
            "rotate",
            "scale",
            "smooth",
            "tidy_line",
            "trim"
        ]
    );
}

#[test]
fn every_field_says_what_it_is() {
    let schema = schema();
    let mut count = 0;
    each_object(&schema, &mut |object| {
        for (name, field) in object["properties"].as_object().unwrap() {
            if name == "op" {
                continue;
            }
            // A selection refers to its own definition, and a shape's fields are one level down:
            // each says what it is.
            let said = field.get("description").is_some()
                || field.get("$ref").is_some()
                || field.get("properties").is_some();
            assert!(said, "{name}: {field}");
            count += 1;
        }
    });
    assert!(count > 50, "{count} fields");
}

#[test]
fn orders_and_shapes_say_what_they_do_and_refuse_unknown_fields() {
    let schema = schema();
    let mut objects = 0;
    each_object(&schema, &mut |object| {
        if object["properties"].get("op").is_some() {
            assert!(object.get("description").is_some(), "{object}");
            assert_eq!(object["additionalProperties"], false, "{object}");
            objects += 1;
        }
    });
    assert_eq!(objects, 14);
    let definitions = &schema["$defs"];
    for name in [
        "Select", "Shape", "Part", "PenTool", "Flip", "TidyMode", "TidyGaps",
    ] {
        let definition = &definitions[name];
        assert!(
            definition.get("description").is_some(),
            "{name}: {definition}"
        );
    }
    assert_eq!(definitions["Select"]["additionalProperties"], false);
    assert_eq!(definitions["Part"]["additionalProperties"], false);
}

#[test]
fn the_orders_of_the_design_document_can_be_read() {
    let orders = parse(
        r##"[
        {"op":"recolor","select":{"rect":[0,0,10,10],"color":"#000000","layer":0},"color":"#c0392b"},
        {"op":"restyle","select":{"min_size":1,"max_size":5,"not":[1]},"width":0.5,"tool":"feltTip"},
        {"op":"delete","select":{"ids":[3]}},
        {"op":"move","select":{},"dx":1,"dy":-1},
        {"op":"scale","select":{},"factor":0.9,"origin":[10,10]},
        {"op":"scale","select":{},"sx":0.9,"sy":1.1,"origin":[10,10]},
        {"op":"rotate","select":{},"degrees":-2.5,"origin":[10,10]},
        {"op":"smooth","select":{},"sigma":0.2},
        {"op":"trim","stroke":0,"from":10,"to":20},
        {"op":"replace","select":{"ids":[1]},"color":"#000000","width":0.4,"with":[
            {"line":{"from":[0,0],"to":[1,1]}},
            {"polyline":{"points":[[0,0],[1,1]],"closed":true}},
            {"rect":{"x":0,"y":0,"w":1,"h":1}},
            {"circle":{"c":[0,0],"r":1}},
            {"ellipse":{"c":[0,0],"rx":1,"ry":2,"angle":10}},
            {"arrow":{"from":[0,0],"to":[1,1],"head":2,"both":true}},
            {"dashed":{"from":[0,0],"to":[1,1],"dash":1,"gap":1,"head":2}},
            {"curve":{"points":[[0,0],[1,1],[2,0]]}}]},
        {"op":"add","shapes":[{"line":{"from":[0,0],"to":[1,1]}}],"color":"#ffee00","width":3,
         "tool":"highlighter","layer":0,"behind":true,"after":2},
        {"op":"copy","select":{"ids":[0]},"to_x":3,"dy":1,"factor":1.2,"color":"#000000","after":2},
        {"op":"copy","select":{"ids":[0]},"dx":3},
        {"op":"glyph","color":"#000000","after":2,"parts":[
            {"stroke":0,"from":1,"to":5,"flip":"rot","sx":1.2,"sy":0.8,"x":10,"y":20}]},
        {"op":"retime","select":{"ids":[1]},"after":2},
        {"op":"tidy_line","select":{"ids":[0,1]},"baseline":40,"left":30,"mode":"rigid","scale":0.9,
         "slope":1.5,"gaps":"keep","word_gap":3.2,"smooth":0.2},
        {"op":"tidy_line","select":{},"mode":"words","gaps":"even"}]"##,
    );
    assert_eq!(orders.len(), 17);
    let ops: Vec<&str> = orders.iter().map(Order::op).collect();
    assert_eq!(ops[0], "recolor");
    assert_eq!(ops[15], "tidy_line");
}
