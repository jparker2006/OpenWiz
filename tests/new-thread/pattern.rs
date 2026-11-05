use serde_json::Value;

const KEYWORDS: &[&str] = &["pattern", "maxUtf8Bytes", "uniqueItems", "minItems"];

/// The keywords that only an array node may carry (B2).
const ARRAY_KEYWORDS: &[&str] = &["uniqueItems", "minItems"];

#[derive(Debug)]
enum Atom {
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
        repeat: bool,
    },
}

fn escape(chars: &[char], i: &mut usize) -> Result<char, String> {
    // At `\`: only `\uXXXX`.
    let hex: String = chars
        .get(*i + 2..*i + 6)
        .ok_or("a short escape")?
        .iter()
        .collect();
    if chars.get(*i + 1) != Some(&'u') || hex.len() != 4 {
        return Err("an escape other than \\uXXXX".to_owned());
    }
    let code = u32::from_str_radix(&hex, 16).map_err(|_| "a bad \\u escape")?;
    *i += 6;
    char::from_u32(code).ok_or_else(|| "a surrogate escape".to_owned())
}

const SPECIAL: &[char] = &[
    '.', '|', '?', '*', '+', '(', ')', '{', '}', '^', '$', '[', ']',
];

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
        let (negated, ranges) = match chars[i] {
            '[' => {
                i += 1;
                let negated = chars.get(i) == Some(&'^');
                if negated {
                    i += 1;
                }
                let mut ranges = Vec::new();
                loop {
                    let lo = match chars.get(i) {
                        None => return Err("an unclosed class".to_owned()),
                        Some(']') => {
                            i += 1;
                            break;
                        }
                        Some('[') | Some('^') => return Err("[ or ^ inside a class".to_owned()),
                        Some('\\') => escape(&chars, &mut i)?,
                        Some(&c) => {
                            i += 1;
                            c
                        }
                    };
                    if chars.get(i) != Some(&'-') || chars.get(i + 1) == Some(&']') {
                        ranges.push((lo, lo));
                        continue;
                    }
                    i += 1;
                    let hi = match chars.get(i) {
                        None => return Err("an open range".to_owned()),
                        Some('\\') => escape(&chars, &mut i)?,
                        Some(&c) => {
                            i += 1;
                            c
                        }
                    };
                    if lo > hi {
                        return Err("a reversed range".to_owned());
                    }
                    ranges.push((lo, hi));
                }
                if ranges.is_empty() {
                    return Err("an empty class".to_owned());
                }
                (negated, ranges)
            }
            '\\' => {
                let c = escape(&chars, &mut i)?;
                (false, vec![(c, c)])
            }
            c if SPECIAL.contains(&c) => return Err(format!("{c:?} outside a class")),
            c => {
                i += 1;
                (false, vec![(c, c)])
            }
        };
        let repeat = chars.get(i) == Some(&'*');
        if repeat {
            i += 1;
        }
        atoms.push(Atom::Class {
            negated,
            ranges,
            repeat,
        });
    }
    Ok(atoms)
}

fn fits(atom: &Atom, c: char) -> bool {
    let Atom::Class {
        negated, ranges, ..
    } = atom;
    ranges.iter().any(|&(lo, hi)| lo <= c && c <= hi) != *negated
}

fn matches(atoms: &[Atom], text: &[char]) -> bool {
    match atoms.split_first() {
        None => text.is_empty(),
        Some((atom, rest)) => {
            let Atom::Class { repeat, .. } = atom;
            if *repeat {
                let run = text.iter().take_while(|&&c| fits(atom, c)).count();
                (0..=run).rev().any(|n| matches(rest, &text[n..]))
            } else {
                text.first().is_some_and(|&c| fits(atom, c)) && matches(rest, &text[1..])
            }
        }
    }
}

/// A parsed pattern, to match many texts.
pub struct Pattern(Vec<Atom>);

impl Pattern {
    /// `pattern`, which must be of the supported form.
    pub fn new(pattern: &str) -> Self {
        Self(parse(pattern).unwrap_or_else(|e| panic!("pattern {pattern:?}: {e}")))
    }

    pub fn is_match(&self, text: &str) -> bool {
        matches(&self.0, &text.chars().collect::<Vec<_>>())
    }
}

/// Whether `text` matches `pattern`, which must be of the supported form.
pub fn is_match(pattern: &str, text: &str) -> bool {
    Pattern::new(pattern).is_match(text)
}

/// `schema` without this module's keywords, for the shared checker.
fn stripped(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !KEYWORDS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), stripped(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(stripped).collect()),
        other => other.clone(),
    }
}

fn lint_node(node: &Value, path: &str, errors: &mut Vec<String>) {
    match node {
        Value::Object(map) => {
            for key in KEYWORDS {
                let (kind, on) = if ARRAY_KEYWORDS.contains(key) {
                    ("array", map.get("type") == Some(&Value::from("array")))
                } else {
                    ("string", map.get("type") == Some(&Value::from("string")))
                };
                if map.contains_key(*key) && !on {
                    errors.push(format!("{path}/{key}: only on a \"type\": \"{kind}\" node"));
                }
            }
            if let Some(u) = map.get("uniqueItems")
                && u != &Value::Bool(true)
            {
                errors.push(format!("{path}/uniqueItems: only true"));
            }
            if let Some(n) = map.get("minItems")
                && n.as_u64().is_none()
            {
                errors.push(format!("{path}/minItems: not a count"));
            }
            if let Some(p) = map.get("pattern") {
                match p.as_str().map(parse) {
                    Some(Ok(_)) => {}
                    Some(Err(e)) => errors.push(format!("{path}/pattern: unsupported ({e})")),
                    None => errors.push(format!("{path}/pattern: not a string")),
                }
            }
            if let Some(n) = map.get("maxUtf8Bytes")
                && n.as_u64().is_none()
            {
                errors.push(format!("{path}/maxUtf8Bytes: not a count"));
            }
            for (k, child) in map {
                if !KEYWORDS.contains(&k.as_str()) {
                    lint_node(child, &format!("{path}/{k}"), errors);
                }
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                lint_node(item, &format!("{path}/{i}"), errors);
            }
        }
        _ => {}
    }
}

/// Every keyword the shared checker or this module would not enforce.
pub fn lint(schema: &Value) -> Vec<String> {
    let mut errors = crate::schema::lint(&stripped(schema));
    lint_node(schema, "#", &mut errors);
    errors
}

/// `value` against `schema` (rooted at `root`, at JSON pointer `at`): the
/// shared checker on the stripped schema, then this module's keywords.
pub fn validate(schema: &Value, value: &Value) -> Vec<String> {
    let mut errors = crate::schema::validate(&stripped(schema), value);
    if errors.is_empty() {
        walk(schema, schema, value, "$", &mut errors);
    }
    errors
}

/// Whether `value` matches the node at `pointer` structurally (the shared
/// checker, keywords stripped, `$ref`s resolved against `root`).
fn structurally(root: &Value, pointer: &str, value: &Value) -> bool {
    let mut probe = stripped(root);
    let object = probe.as_object_mut().expect("an object schema");
    for key in [
        "type",
        "properties",
        "required",
        "oneOf",
        "items",
        "const",
        "enum",
    ] {
        object.remove(key);
    }
    object.insert("$ref".to_owned(), Value::from(format!("#{pointer}")));
    crate::schema::validate(&probe, value).is_empty()
}

fn walk(root: &Value, node: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    walk_at(root, node, None, value, path, errors)
}

fn walk_at(
    root: &Value,
    node: &Value,
    pointer: Option<&str>,
    value: &Value,
    path: &str,
    errors: &mut Vec<String>,
) {
    let Some(rules) = node.as_object() else {
        return;
    };
    if let Some(reference) = rules.get("$ref").and_then(Value::as_str) {
        let target = reference.strip_prefix('#').unwrap_or_default();
        if let Some(next) = root.pointer(target) {
            walk_at(root, next, Some(target), value, path, errors);
        }
    }
    if let Some(text) = value.as_str() {
        if let Some(p) = rules.get("pattern").and_then(Value::as_str)
            && !is_match(p, text)
        {
            errors.push(format!("{path}: does not match {p:?}"));
        }
        if let Some(max) = rules.get("maxUtf8Bytes").and_then(Value::as_u64)
            && text.len() as u64 > max
        {
            errors.push(format!(
                "{path}: {} bytes of UTF-8, above {max}",
                text.len()
            ));
        }
    }
    if let (Some(props), Some(object)) = (
        rules.get("properties").and_then(Value::as_object),
        value.as_object(),
    ) {
        for (k, child) in object {
            if let Some(schema) = props.get(k) {
                let at = pointer.map(|p| format!("{p}/properties/{k}"));
                walk_at(
                    root,
                    schema,
                    at.as_deref(),
                    child,
                    &format!("{path}.{k}"),
                    errors,
                );
            }
        }
    }
    if let Some(array) = value.as_array() {
        if let Some(min) = rules.get("minItems").and_then(Value::as_u64)
            && (array.len() as u64) < min
        {
            errors.push(format!("{path}: {} items, fewer than {min}", array.len()));
        }
        if rules.get("uniqueItems") == Some(&Value::Bool(true)) {
            for (i, item) in array.iter().enumerate() {
                if array[..i].contains(item) {
                    errors.push(format!("{path}[{i}]: equal to an earlier item"));
                }
            }
        }
    }
    if let (Some(items), Some(array)) = (rules.get("items"), value.as_array()) {
        for (i, item) in array.iter().enumerate() {
            let at = pointer.map(|p| format!("{p}/items"));
            walk_at(
                root,
                items,
                at.as_deref(),
                item,
                &format!("{path}[{i}]"),
                errors,
            );
        }
    }
    if let Some(branches) = rules.get("oneOf").and_then(Value::as_array) {
        let Some(base) = pointer else {
            errors.push(format!("{path}: a oneOf with no pointer to check it by"));
            return;
        };
        let matched: Vec<usize> = (0..branches.len())
            .filter(|i| structurally(root, &format!("{base}/oneOf/{i}"), value))
            .collect();
        if let [one] = matched.as_slice() {
            let at = format!("{base}/oneOf/{one}");
            walk_at(root, &branches[*one], Some(&at), value, path, errors);
        }
    }
}

#[test]
fn the_pattern_form_is_checked_and_matched() {
    for bad in [
        "a", "^a", "^.$", "^\\s$", "^[a-]$x", "^[]$", "^(a)$", "^[z-a]$", "^a+$",
    ] {
        assert!(parse(bad).is_err(), "{bad:?} parsed");
    }
    assert!(is_match("^[A-Za-z0-9][A-Za-z0-9_-]*$", "op-1"));
    assert!(!is_match("^[A-Za-z0-9][A-Za-z0-9_-]*$", "-op"));
    assert!(is_match("^ow1-[0-9a-f]*$", "ow1-0af"));
    assert!(!is_match("^ow1-[0-9a-f]*$", "ow2-0af"));
    assert!(is_match("^[^\\u0000]*$", "a\u{1F9EA}b"));
    assert!(!is_match("^[^\\u0000]*$", "a\u{0}b"));
    assert!(is_match("^[\\u0041-\\u0043]*$", "ABC"));
}
