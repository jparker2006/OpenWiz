//! A small JSON Schema checker for `contracts/inventory.v1.schema.json`.
//!
//! No JSON Schema crate is approved, so this checks the subset the schema
//! uses: `type`, `required`, `properties`, `additionalProperties: false`,
//! `items`, `enum`, `const`, `oneOf`, local `$ref`, `minimum`, `minLength`
//! and `maxLength`. [`lint`] walks the whole schema, every `$defs` entry
//! included. It refuses any other keyword, any other form of
//! `additionalProperties` and any `$ref` that does not resolve, so the
//! schema cannot rely on a rule this checker ignores.

use serde_json::Value;

pub const SCHEMA_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../contracts/inventory.v1.schema.json"
);

const KNOWN_KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "$defs",
    "$ref",
    "title",
    "description",
    "type",
    "required",
    "properties",
    "additionalProperties",
    "items",
    "enum",
    "const",
    "oneOf",
    "minimum",
    "minLength",
    "maxLength",
];

pub fn load_schema() -> Value {
    let text = std::fs::read_to_string(SCHEMA_PATH).expect("schema file is readable");
    serde_json::from_str(&text).expect("schema is JSON")
}

/// Returns every keyword or form in `schema` that this checker does not
/// enforce, wherever it appears.
pub fn lint(schema: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    lint_node(schema, schema, "#", &mut errors);
    errors
}

fn lint_node(root: &Value, node: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(rules) = node.as_object() else {
        errors.push(format!("{path}: schema node is not an object"));
        return;
    };
    for (key, child) in rules {
        let at = format!("{path}/{key}");
        match key.as_str() {
            k if !KNOWN_KEYWORDS.contains(&k) => {
                errors.push(format!("{path}: unsupported schema keyword {k}"))
            }
            "additionalProperties" if child != &Value::Bool(false) => {
                errors.push(format!("{at}: only `false` is checked"))
            }
            "$ref" => match child.as_str() {
                Some(reference) if resolve(root, reference).is_some() => {}
                _ => errors.push(format!("{at}: unresolvable $ref {child}")),
            },
            "$defs" | "properties" => {
                for (name, sub) in child.as_object().into_iter().flatten() {
                    lint_node(root, sub, &format!("{at}/{name}"), errors);
                }
            }
            "items" => lint_node(root, child, &at, errors),
            "oneOf" => {
                for (i, sub) in child.as_array().into_iter().flatten().enumerate() {
                    lint_node(root, sub, &format!("{at}/{i}"), errors);
                }
            }
            _ => {}
        }
    }
}

/// Returns every violation of `schema` by `value`, with its JSON path.
pub fn validate(schema: &Value, value: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    check(schema, schema, value, "$", &mut errors);
    errors
}

fn check(root: &Value, schema: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(rules) = schema.as_object() else {
        errors.push(format!("{path}: schema node is not an object"));
        return;
    };
    for key in rules.keys() {
        if !KNOWN_KEYWORDS.contains(&key.as_str()) {
            errors.push(format!("{path}: unsupported schema keyword {key}"));
        }
    }

    if let Some(reference) = rules.get("$ref").and_then(Value::as_str) {
        match resolve(root, reference) {
            Some(target) => check(root, target, value, path, errors),
            None => errors.push(format!("{path}: unresolvable $ref {reference}")),
        }
    }

    if let Some(types) = rules.get("type") {
        let allowed: Vec<&str> = match types {
            Value::String(t) => vec![t.as_str()],
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !allowed.iter().any(|t| type_matches(t, value)) {
            errors.push(format!("{path}: expected type {allowed:?}, got {value}"));
            return;
        }
    }

    if let Some(expected) = rules.get("const")
        && value != expected
    {
        errors.push(format!("{path}: expected {expected}, got {value}"));
    }
    if let Some(options) = rules.get("enum").and_then(Value::as_array)
        && !options.contains(value)
    {
        errors.push(format!("{path}: {value} is not one of {options:?}"));
    }
    if let (Some(min), Some(n)) = (rules.get("minimum").and_then(Value::as_f64), value.as_f64())
        && n < min
    {
        errors.push(format!("{path}: {n} is below the minimum {min}"));
    }
    if let (Some(min), Some(s)) = (
        rules.get("minLength").and_then(Value::as_u64),
        value.as_str(),
    ) && (s.chars().count() as u64) < min
    {
        errors.push(format!("{path}: shorter than {min} characters"));
    }
    if let (Some(max), Some(s)) = (
        rules.get("maxLength").and_then(Value::as_u64),
        value.as_str(),
    ) && s.chars().count() as u64 > max
    {
        errors.push(format!("{path}: longer than {max} characters"));
    }

    if let Some(options) = rules.get("oneOf").and_then(Value::as_array) {
        let passing = options
            .iter()
            .filter(|option| {
                let mut e = Vec::new();
                check(root, option, value, path, &mut e);
                e.is_empty()
            })
            .count();
        if passing != 1 {
            errors.push(format!(
                "{path}: matches {passing} of the oneOf options, not exactly one"
            ));
        }
    }

    if let Some(object) = value.as_object() {
        let properties = rules.get("properties").and_then(Value::as_object);
        if let Some(required) = rules.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    errors.push(format!("{path}: missing required field {key}"));
                }
            }
        }
        for (key, child) in object {
            let child_path = format!("{path}.{key}");
            match properties.and_then(|p| p.get(key)) {
                Some(child_schema) => check(root, child_schema, child, &child_path, errors),
                None => {
                    if rules.get("additionalProperties") == Some(&Value::Bool(false)) {
                        errors.push(format!("{path}: unexpected field {key}"));
                    }
                }
            }
        }
    }

    if let (Some(items), Some(array)) = (rules.get("items"), value.as_array()) {
        for (i, item) in array.iter().enumerate() {
            check(root, items, item, &format!("{path}[{i}]"), errors);
        }
    }
}

fn type_matches(t: &str, value: &Value) -> bool {
    match t {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        _ => false,
    }
}

fn resolve<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    root.pointer(pointer)
}
