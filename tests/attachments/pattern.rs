//! `pattern` for the attachment schema .
//!
//! The shared checker (`tests/support/schema.rs`) has no `pattern`, and is
//! outside B1's owned paths, so this module adds it for the attachment
//! schema only. [`lint`] and [`validate`] run the shared checker on the
//! schema with every `pattern` taken out, then check the patterns here.
//!
//! Only one form of pattern is supported, so none can mean more than this
//! checker enforces: `^`, then character classes (`[A-Za-z0-9._-]`, ranges
//! and single characters, a `-` only last), each optionally followed by
//! `*`, then `$`. A pattern sits only on a `"type": "string"` node, and is
//! never reachable from a `oneOf` branch (where it would also apply to the
//! branches that don't match).

use std::collections::BTreeSet;

use serde_json::Value;

/// One `[...]` class, and whether `*` follows it.
struct Atom {
    class: Vec<(char, char)>,
    repeat: bool,
}

/// Parses a pattern of the supported form, or says why it isn't one.
fn parse(pattern: &str) -> Result<Vec<Atom>, String> {
    let body = pattern
        .strip_prefix('^')
        .and_then(|p| p.strip_suffix('$'))
        .ok_or("not anchored with ^ and $")?;
    let chars: Vec<char> = body.chars().collect();
    let mut atoms = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '[' {
            return Err(format!("{:?} outside a class", chars[i]));
        }
        let close = chars[i..]
            .iter()
            .position(|&c| c == ']')
            .ok_or("an unclosed class")?
            + i;
        let inner = &chars[i + 1..close];
        if inner.is_empty() || inner[0] == '^' {
            return Err("an empty or negated class".to_owned());
        }
        let mut class = Vec::new();
        let mut j = 0;
        while j < inner.len() {
            let c = inner[j];
            if matches!(c, '\\' | '[') {
                return Err(format!("{c:?} in a class"));
            }
            if j + 2 < inner.len() && inner[j + 1] == '-' {
                if c > inner[j + 2] {
                    return Err("a reversed range".to_owned());
                }
                class.push((c, inner[j + 2]));
                j += 3;
            } else if c == '-' && j + 1 != inner.len() {
                return Err("a - that isn't last".to_owned());
            } else {
                class.push((c, c));
                j += 1;
            }
        }
        i = close + 1;
        let repeat = chars.get(i) == Some(&'*');
        if repeat {
            i += 1;
        }
        atoms.push(Atom { class, repeat });
    }
    Ok(atoms)
}

fn matches(atoms: &[Atom], text: &[char]) -> bool {
    match atoms.split_first() {
        None => text.is_empty(),
        Some((atom, rest)) => {
            let fits = |c: &char| atom.class.iter().any(|&(lo, hi)| lo <= *c && *c <= hi);
            if atom.repeat {
                let run = text.iter().take_while(|c| fits(c)).count();
                (0..=run).rev().any(|n| matches(rest, &text[n..]))
            } else {
                text.first().is_some_and(fits) && matches(rest, &text[1..])
            }
        }
    }
}

/// Whether `text` matches `pattern`, which must be of the supported form.
pub fn is_match(pattern: &str, text: &str) -> bool {
    let atoms = parse(pattern).unwrap_or_else(|e| panic!("pattern {pattern}: {e}"));
    matches(&atoms, &text.chars().collect::<Vec<_>>())
}

/// `schema` with every `pattern` keyword removed, for the shared checker.
fn without_patterns(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| k.as_str() != "pattern")
                .map(|(k, v)| (k.clone(), without_patterns(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_patterns).collect()),
        other => other.clone(),
    }
}

fn resolve<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    root.pointer(reference.strip_prefix('#')?)
}

/// Whether a `pattern` can be reached from `node`, following `$ref`.
fn reaches_pattern(root: &Value, node: &Value, seen: &mut BTreeSet<String>) -> bool {
    match node {
        Value::Object(map) => map.iter().any(|(key, child)| match key.as_str() {
            "pattern" => true,
            "$ref" => {
                let reference = child.as_str().unwrap_or_default().to_owned();
                seen.insert(reference.clone())
                    && resolve(root, &reference).is_some_and(|t| reaches_pattern(root, t, seen))
            }
            _ => reaches_pattern(root, child, seen),
        }),
        Value::Array(items) => items.iter().any(|v| reaches_pattern(root, v, seen)),
        _ => false,
    }
}

fn lint_patterns(root: &Value, node: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(map) = node.as_object() else { return };
    if let Some(pattern) = map.get("pattern") {
        match pattern.as_str().map(parse) {
            Some(Ok(_)) => {}
            Some(Err(e)) => errors.push(format!("{path}/pattern: unsupported ({e})")),
            None => errors.push(format!("{path}/pattern: not a string")),
        }
        if map.get("type") != Some(&Value::String("string".to_owned())) {
            errors.push(format!("{path}/pattern: only on a string node"));
        }
    }
    if let Some(branches) = map.get("oneOf")
        && reaches_pattern(root, branches, &mut BTreeSet::new())
    {
        errors.push(format!("{path}/oneOf: reaches a pattern"));
    }
    for (key, child) in map {
        match child {
            Value::Object(_) => lint_patterns(root, child, &format!("{path}/{key}"), errors),
            Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    lint_patterns(root, item, &format!("{path}/{key}/{i}"), errors);
                }
            }
            _ => {}
        }
    }
}

/// The shared lint on the schema without its patterns, then the patterns'.
pub fn lint(schema: &Value) -> Vec<String> {
    let mut errors = crate::schema::lint(&without_patterns(schema));
    lint_patterns(schema, schema, "#", &mut errors);
    errors
}

fn check_patterns(
    root: &Value,
    schema: &Value,
    value: &Value,
    path: &str,
    errors: &mut Vec<String>,
) {
    let Some(rules) = schema.as_object() else {
        return;
    };
    if let Some(target) = rules
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| resolve(root, r))
    {
        check_patterns(root, target, value, path, errors);
    }
    if let (Some(pattern), Some(text)) =
        (rules.get("pattern").and_then(Value::as_str), value.as_str())
        && !is_match(pattern, text)
    {
        errors.push(format!("{path}: does not match {pattern}"));
    }
    if let (Some(properties), Some(object)) = (
        rules.get("properties").and_then(Value::as_object),
        value.as_object(),
    ) {
        for (key, child) in object {
            if let Some(child_schema) = properties.get(key) {
                check_patterns(root, child_schema, child, &format!("{path}.{key}"), errors);
            }
        }
    }
    if let (Some(items), Some(array)) = (rules.get("items"), value.as_array()) {
        for (i, item) in array.iter().enumerate() {
            check_patterns(root, items, item, &format!("{path}[{i}]"), errors);
        }
    }
}

/// The shared checker's violations, then the patterns'.
pub fn validate(schema: &Value, value: &Value) -> Vec<String> {
    let mut errors = crate::schema::validate(&without_patterns(schema), value);
    check_patterns(schema, schema, value, "$", &mut errors);
    errors
}
