//! The skills folder (OpenWiz 0.0.5, the implementation): the eight fleet skills
//! install in one step, and any agent, in any harness, can invoke them.
//!
//! Rules, checked on the repository and on planted breaks:
//! 1. **The eight, exactly.** `skills/` holds `README.md` and one real folder
//!    for each name in [`SKILLS`], and no links anywhere (Codex drops links
//!    when it installs a plugin).
//! 2. **Frontmatter.** Each `SKILL.md` opens with `name` (its folder's name)
//!    and `description`, one line each, and no other key. It is an allowlist,
//!    so no harness's user-only flag (`disable-model-invocation`) gets in.
//! 3. **Codex.** `agents/openai.yaml`, where present, holds only
//!    `interface.display_name` and `interface.short_description`, never a
//!    `policy` (Codex's user-only switch).
//! 4. **Matt's MIT notice.** All eight adapt Matt Pocock's skills, so a
//!    written skill (any body but the exact [`STUB`]) carries his copyright
//!    and MIT notice word for word, and has its `agents/openai.yaml`.
//! 5. **Links resolve.** `.agents/skills/<name>` and `.agents/skills/<name>`
//!    are links to `../../skills/<name>`, every link in those two folders is
//!    one of them, and every relative Markdown link under `skills/` resolves
//!    inside the repository.
//! 6. **Manifests.** `.claude-plugin/plugin.json` lists exactly
//!    `./skills/<name>` for the eight, beside metadata only, and
//!    `.claude-plugin/marketplace.json` offers that one plugin from `./`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, FileType};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// The eight skills of spec the implementation. Every one adapts Matt Pocock's.
const SKILLS: [&str; 8] = [
    "grill",
    "plan-graph",
    "launch-fleet",
    "build-ticket",
    "review-ticket",
    "handoff",
    "setup",
    "triage",
];

/// Where agents look for skills inside a repository.
const LINK_DIRS: [&str; 1] = [".agents/skills"];

/// The only frontmatter keys a skill may have.
const FRONTMATTER_KEYS: [&str; 2] = ["name", "description"];

/// The longest description the Agent Skills format allows.
const DESCRIPTION_MAX_CHARS: usize = 1024;

/// The only keys under `interface:` in `agents/openai.yaml`.
const OPENAI_KEYS: [&str; 2] = ["display_name", "short_description"];

/// The only keys in `plugin.json`: metadata and `skills`. No `version`, so
/// installs follow commits; nothing that ships hooks, commands or servers.
const PLUGIN_KEYS: [&str; 8] = [
    "name",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "skills",
];

/// The only keys in `marketplace.json`, and in its one plugin entry.
const MARKETPLACE_KEYS: [&str; 4] = ["name", "owner", "description", "plugins"];
const MARKETPLACE_ENTRY_KEYS: [&str; 5] = ["name", "source", "description", "category", "keywords"];

/// The whole body of a skill that is not written yet.
const STUB: &str = "**Not written yet.** This file only fixes the skill's name and path; \
its implementation writes it. Stop here, and tell whoever invoked it that this skill \
is not ready.";

/// How a stub's body starts; a written skill drops it.
const STUB_MARK: &str = "**Not written yet.**";

/// Matt Pocock's copyright and MIT notice, line for line from `LICENSE` in
/// `github.com/mattpocock/skills` @ `a496f55`. Compared with whitespace
/// collapsed, so line breaks don't matter.
const MATT_NOTICE: [&str; 16] = [
    "Copyright (c) 2026 Matt Pocock",
    "Permission is hereby granted, free of charge, to any person obtaining a copy",
    "of this software and associated documentation files (the \"Software\"), to deal",
    "in the Software without restriction, including without limitation the rights",
    "to use, copy, modify, merge, publish, distribute, sublicense, and/or sell",
    "copies of the Software, and to permit persons to whom the Software is",
    "furnished to do so, subject to the following conditions:",
    "The above copyright notice and this permission notice shall be included in all",
    "copies or substantial portions of the Software.",
    "THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR",
    "IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,",
    "FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE",
    "AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER",
    "LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,",
    "OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE",
    "SOFTWARE.",
];

/// Every rule the repository at `root` breaks, one line each.
fn problems(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    check_folder(root, &mut out);
    for name in SKILLS {
        check_skill(root, name, &mut out);
    }
    check_plugin(root, &mut out);
    check_marketplace(root, &mut out);
    check_links(root, &mut out);
    check_markdown(root, &mut out);
    out
}

// ---- 1. The eight, exactly ----

fn check_folder(root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root.join("skills")) else {
        out.push("skills/ is missing".into());
        return;
    };
    let mut found = BTreeSet::new();
    for entry in entries {
        let entry = entry.expect("entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().expect("file type");
        let readme = name == "README.md" && kind.is_file();
        if readme || (kind.is_dir() && SKILLS.contains(&name.as_str())) {
            found.insert(name);
        } else {
            out.push(format!(
                "skills/{name}: only README.md and the eight skill folders belong here"
            ));
        }
    }
    for name in ["README.md"].into_iter().chain(SKILLS) {
        if !found.contains(name) {
            out.push(format!("skills/{name} is missing"));
        }
    }
}

// ---- 2 to 4. Each skill ----

fn check_skill(root: &Path, name: &str, out: &mut Vec<String>) {
    let dir = root.join("skills").join(name);
    let at = format!("skills/{name}/SKILL.md");
    let Ok(text) = fs::read_to_string(dir.join("SKILL.md")) else {
        out.push(format!("{at} is missing"));
        return;
    };
    let Some((front, body)) = split_frontmatter(&text) else {
        out.push(format!(
            "{at}: it must open with a `---` line, then the frontmatter, closed by a `---` line"
        ));
        return;
    };
    check_frontmatter(&at, name, &front, out);

    let openai = dir.join("agents/openai.yaml");
    let openai_at = format!("skills/{name}/agents/openai.yaml");
    let has_openai = match fs::read_to_string(&openai) {
        Ok(yaml) => {
            check_openai_yaml(&openai_at, &yaml, out);
            true
        }
        Err(_) => false,
    };

    if body.trim() == STUB {
        return;
    }
    if body.contains(STUB_MARK) {
        out.push(format!(
            "{at}: a written skill drops the stub line `{STUB_MARK}`"
        ));
    }
    if !collapse(&body).contains(&collapse(&MATT_NOTICE.join("\n"))) {
        out.push(format!(
            "{at}: a written skill carries Matt Pocock's copyright and MIT notice word for word \
             (all eight adapt his skills; see skills/README.md)"
        ));
    }
    if !has_openai {
        out.push(format!(
            "{openai_at} is missing: a written skill has one for Codex"
        ));
    }
}

/// The frontmatter lines and the body, or `None` when the file does not
/// open with a `---` line or never closes its frontmatter.
fn split_frontmatter(text: &str) -> Option<(Vec<&str>, String)> {
    let mut lines = text.split('\n');
    if lines.next()? != "---" {
        return None;
    }
    let mut front = Vec::new();
    for line in lines.by_ref() {
        if line == "---" {
            return Some((front, lines.collect::<Vec<_>>().join("\n")));
        }
        front.push(line);
    }
    None
}

fn check_frontmatter(at: &str, name: &str, front: &[&str], out: &mut Vec<String>) {
    let mut values = BTreeMap::new();
    for line in front {
        let Some((key, rest)) = line.split_once(':').filter(|(k, _)| is_key(k)) else {
            out.push(format!(
                "{at}: frontmatter line `{line}` is not a one-line `key: value`"
            ));
            continue;
        };
        if !FRONTMATTER_KEYS.contains(&key) {
            out.push(format!(
                "{at}: frontmatter key `{key}` is not allowed: only `name` and `description`, \
                 so no harness can make the skill user-only (`disable-model-invocation`)"
            ));
            continue;
        }
        match scalar(rest) {
            Ok(value) => {
                if values.insert(key, value).is_some() {
                    out.push(format!("{at}: `{key}` appears twice"));
                }
            }
            Err(why) => out.push(format!("{at}: `{key}` {why}")),
        }
    }
    match values.get("name") {
        None => out.push(format!("{at}: the frontmatter has no `name`")),
        Some(found) if found != name => out.push(format!(
            "{at}: `name` is `{found}`, not its folder's name `{name}`"
        )),
        Some(_) => {}
    }
    match values.get("description") {
        None => out.push(format!("{at}: the frontmatter has no `description`")),
        Some(text) if text.chars().count() > DESCRIPTION_MAX_CHARS => out.push(format!(
            "{at}: `description` is longer than {DESCRIPTION_MAX_CHARS} characters"
        )),
        Some(_) => {}
    }
}

fn is_key(key: &str) -> bool {
    key.starts_with(|c: char| c.is_ascii_lowercase())
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// A one-line YAML value, after its key's `:`: a plain scalar YAML reads as
/// the same text, or a quoted one. Anything else is refused, not guessed at.
fn scalar(rest: &str) -> Result<String, &'static str> {
    let raw = rest.trim();
    if raw.is_empty() {
        return Err("needs a value on the same line");
    }
    if !rest.starts_with(' ') {
        return Err("needs a space after its `:`");
    }
    let value = if let Some(inner) = raw.strip_prefix('"') {
        let inner = inner
            .strip_suffix('"')
            .ok_or("opens a double quote it does not close")?;
        let mut value = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some(e @ ('"' | '\\')) => value.push(e),
                    _ => return Err("uses an escape other than \\\" or \\\\"),
                },
                '"' => return Err("has an unescaped double quote inside"),
                c => value.push(c),
            }
        }
        value
    } else if let Some(inner) = raw.strip_prefix('\'') {
        let inner = inner
            .strip_suffix('\'')
            .ok_or("opens a single quote it does not close")?;
        if inner.replace("''", "").contains('\'') {
            return Err("has an unescaped single quote inside");
        }
        inner.replace("''", "'")
    } else {
        const INDICATORS: &[char] = &[
            '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '%', '@', '`',
        ];
        if raw.starts_with(INDICATORS) {
            return Err("starts with a YAML indicator: quote it");
        }
        if raw.contains(": ") || raw.contains(" #") || raw.ends_with(':') {
            return Err("has `: `, ` #` or a final `:`, which YAML misreads: quote it");
        }
        let lower = raw.to_ascii_lowercase();
        let special = ["true", "false", "null", "~", "yes", "no", "on", "off"];
        if special.contains(&lower.as_str()) || raw.parse::<f64>().is_ok() {
            return Err("reads as a YAML boolean, null or number: quote it");
        }
        raw.to_string()
    };
    if value.trim().is_empty() {
        return Err("is empty");
    }
    Ok(value)
}

fn check_openai_yaml(at: &str, text: &str, out: &mut Vec<String>) {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut rest = lines.as_slice();
    match rest.split_first() {
        Some((&"interface:", tail)) => rest = tail,
        _ => out.push(format!("{at}: it must open with `interface:`")),
    }
    let mut seen = BTreeSet::new();
    for line in rest {
        let entry = line
            .strip_prefix("  ")
            .and_then(|l| l.split_once(':'))
            .filter(|(k, _)| OPENAI_KEYS.contains(k));
        match entry {
            Some((key, value)) => {
                if let Err(why) = scalar(value) {
                    out.push(format!("{at}: `{key}` {why}"));
                }
                if !seen.insert(key) {
                    out.push(format!("{at}: `{key}` appears twice"));
                }
            }
            None if line.contains("policy") || line.contains("allow_implicit_invocation") => out
                .push(format!(
                    "{at}: `{}` is not allowed: `policy` is Codex's switch that makes a skill \
                     user-only",
                    line.trim()
                )),
            None => out.push(format!(
                "{at}: `{}` is not allowed: only `interface.display_name` and \
                 `interface.short_description`",
                line.trim()
            )),
        }
    }
    for key in OPENAI_KEYS {
        if !seen.contains(key) {
            out.push(format!("{at}: `interface.{key}` is missing"));
        }
    }
}

/// Text with every run of whitespace collapsed to one space.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- 6. Manifests ----

/// The JSON object at `rel`. A `Value`, so a missing key reads as null.
fn read_object(root: &Path, rel: &str, out: &mut Vec<String>) -> Option<Value> {
    let Ok(text) = fs::read_to_string(root.join(rel)) else {
        out.push(format!("{rel} is missing"));
        return None;
    };
    match serde_json::from_str(&text) {
        Ok(object @ Value::Object(_)) => Some(object),
        Ok(_) => {
            out.push(format!("{rel} must be a JSON object"));
            None
        }
        Err(e) => {
            out.push(format!("{rel} is not JSON: {e}"));
            None
        }
    }
}

fn only_keys(at: &str, object: &Value, allowed: &[&str], out: &mut Vec<String>) {
    for key in object.as_object().into_iter().flat_map(Map::keys) {
        if !allowed.contains(&key.as_str()) {
            out.push(format!(
                "{at}: `{key}` is not allowed (only {})",
                allowed.join(", ")
            ));
        }
    }
}

fn is_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

const PLUGIN: &str = ".claude-plugin/plugin.json";
const MARKETPLACE: &str = ".claude-plugin/marketplace.json";

fn check_plugin(root: &Path, out: &mut Vec<String>) {
    let Some(plugin) = read_object(root, PLUGIN, out) else {
        return;
    };
    only_keys(PLUGIN, &plugin, &PLUGIN_KEYS, out);
    if !plugin["name"].as_str().is_some_and(is_plugin_name) {
        out.push(format!(
            "{PLUGIN}: `name` must be a lowercase-and-hyphens string"
        ));
    }
    if !plugin["description"]
        .as_str()
        .is_some_and(|d| !d.trim().is_empty())
    {
        out.push(format!(
            "{PLUGIN}: `description` must be a non-empty string"
        ));
    }
    let Some(entries) = plugin["skills"].as_array() else {
        out.push(format!(
            "{PLUGIN}: `skills` must be an array of skill folders"
        ));
        return;
    };
    let expected: Vec<String> = SKILLS.iter().map(|n| format!("./skills/{n}")).collect();
    let mut listed = BTreeSet::new();
    for entry in entries {
        let Some(entry) = entry.as_str().filter(|e| expected.iter().any(|x| x == e)) else {
            let shown = entry
                .as_str()
                .map_or_else(|| entry.to_string(), str::to_owned);
            out.push(format!(
                "{PLUGIN}: `{shown}` is not `./skills/<name>` for one of the eight skills"
            ));
            continue;
        };
        if !listed.insert(entry) {
            out.push(format!("{PLUGIN}: `{entry}` is listed twice"));
        }
        if !root.join(entry).join("SKILL.md").is_file() {
            out.push(format!(
                "{PLUGIN}: `{entry}` does not resolve to a folder with a SKILL.md"
            ));
        }
    }
    for entry in &expected {
        if !listed.contains(entry.as_str()) {
            out.push(format!("{PLUGIN}: it does not list `{entry}`"));
        }
    }
}

fn check_marketplace(root: &Path, out: &mut Vec<String>) {
    let Some(market) = read_object(root, MARKETPLACE, out) else {
        return;
    };
    only_keys(MARKETPLACE, &market, &MARKETPLACE_KEYS, out);
    if !market["name"].as_str().is_some_and(is_plugin_name) {
        out.push(format!(
            "{MARKETPLACE}: `name` must be a lowercase-and-hyphens string"
        ));
    }
    if !market["owner"]["name"]
        .as_str()
        .is_some_and(|n| !n.trim().is_empty())
    {
        out.push(format!(
            "{MARKETPLACE}: `owner.name` must be a non-empty string"
        ));
    }
    let entry = match market["plugins"].as_array().map(Vec::as_slice) {
        Some([entry @ Value::Object(_)]) => entry,
        _ => {
            out.push(format!(
                "{MARKETPLACE}: `plugins` must hold exactly one plugin entry, this repository's"
            ));
            return;
        }
    };
    let at = format!("{MARKETPLACE}: `plugins[0]`");
    only_keys(&at, entry, &MARKETPLACE_ENTRY_KEYS, out);
    let plugin_name = fs::read_to_string(root.join(PLUGIN))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|p| p["name"].as_str().map(str::to_owned));
    if entry["name"].as_str() != plugin_name.as_deref() {
        out.push(format!("{at}: `name` must be {PLUGIN}'s `name`"));
    }
    if entry["source"].as_str() != Some("./") || !root.join(PLUGIN).is_file() {
        out.push(format!(
            "{at}: `source` must be `./`, this repository, which holds {PLUGIN}"
        ));
    }
}

// ---- 5. Links ----

fn check_links(root: &Path, out: &mut Vec<String>) {
    for dir in LINK_DIRS {
        for name in SKILLS {
            let link = root.join(dir).join(name);
            let at = format!("{dir}/{name}");
            let expected = Path::new("../../skills").join(name);
            match fs::read_link(&link) {
                Ok(target) if target == expected => {}
                Ok(target) => out.push(format!(
                    "{at} points at `{}`, not `{}`",
                    target.display(),
                    expected.display()
                )),
                Err(_) => {
                    out.push(format!("{at} must be a link to `{}`", expected.display()));
                    continue;
                }
            }
            let real = fs::canonicalize(root.join("skills").join(name)).ok();
            if real.is_none() || fs::canonicalize(&link).ok() != real {
                out.push(format!(
                    "{at} does not resolve to the real folder skills/{name}"
                ));
            }
        }
        let Ok(entries) = fs::read_dir(root.join(dir)) else {
            continue;
        };
        for entry in entries {
            let entry = entry.expect("entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            if SKILLS.contains(&name.as_str()) {
                continue;
            }
            if entry.file_type().expect("file type").is_symlink() {
                out.push(format!(
                    "{dir}/{name} is a link, but not to one of the eight skills"
                ));
            } else if dir == ".agents/skills" {
                out.push(format!(
                    "{dir}/{name}: only the eight links to skills/ belong here"
                ));
            }
        }
    }
}

/// Every path under `dir`, with its own type: links are listed, never
/// followed.
fn tree(dir: &Path) -> Vec<(PathBuf, FileType)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries {
        let entry = entry.expect("entry");
        let kind = entry.file_type().expect("file type");
        if kind.is_dir() {
            out.extend(tree(&entry.path()));
        }
        out.push((entry.path(), kind));
    }
    out
}

fn check_markdown(root: &Path, out: &mut Vec<String>) {
    let Ok(real_root) = fs::canonicalize(root) else {
        return;
    };
    for (path, kind) in tree(&root.join("skills")) {
        let at = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if kind.is_symlink() {
            out.push(format!(
                "{at} is a link: none belong under skills/ (Codex drops them when it installs a plugin)"
            ));
            continue;
        }
        if !kind.is_file() || path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("markdown");
        for target in markdown_targets(&text) {
            if target.contains("://") || target.starts_with("mailto:") {
                continue;
            }
            let file = target.split(['#', '?']).next().unwrap_or_default();
            if file.is_empty() {
                continue;
            }
            match fs::canonicalize(path.parent().expect("parent").join(file)) {
                Ok(real) if real.starts_with(&real_root) => {}
                Ok(_) => out.push(format!("{at}: link `{target}` leaves the repository")),
                Err(_) => out.push(format!("{at}: link `{target}` does not resolve")),
            }
        }
    }
}

/// The targets of the Markdown links and link definitions in `text`, outside
/// code blocks and code spans.
fn markdown_targets(text: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(open) = fence {
            if trimmed.starts_with(open) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f)) {
            fence = Some(open);
            continue;
        }
        let line: String = line.split('`').step_by(2).collect();
        if let Some((_, def)) = line
            .trim_start()
            .strip_prefix('[')
            .and_then(|l| l.split_once("]: "))
        {
            targets.push(first_token(def));
            continue;
        }
        let mut rest = line.as_str();
        while let Some(i) = rest.find("](") {
            rest = &rest[i + 2..];
            let end = rest.find(')').unwrap_or(rest.len());
            targets.push(first_token(&rest[..end]));
            rest = &rest[end..];
        }
    }
    targets
}

fn first_token(text: &str) -> String {
    let token = text.split_whitespace().next().unwrap_or_default();
    token
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

// ---- Tests ----

#[test]
fn the_skills_folder_passes_the_static_check() {
    let found = problems(Path::new(ROOT));
    assert!(
        found.is_empty(),
        "the skills folder breaks the static check:\n{}",
        found.join("\n")
    );
}

/// A good repository in a fresh folder for `case`: the eight stubs, the
/// README, both manifests and both sets of links. Synthetic text only.
fn fixture(case: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("skills-check")
        .join(case);
    let _ = fs::remove_dir_all(&root);
    for name in SKILLS {
        write(&root, &format!("skills/{name}/SKILL.md"), &stub(name));
        for dir in LINK_DIRS {
            crate::dirs::private_dir_all(root.join(dir)).expect("link dir");
            symlink(
                Path::new("../../skills").join(name),
                root.join(dir).join(name),
            )
            .expect("link");
        }
    }
    write(
        &root,
        "skills/README.md",
        "# Synthetic skills\n\nSee [grill](grill/SKILL.md) and `[code](not/a/link.md)`.\n",
    );
    let entries: Vec<String> = SKILLS.iter().map(|n| format!("./skills/{n}")).collect();
    write_json(
        &root,
        PLUGIN,
        &json!({"name": "synthetic-skills", "description": "Synthetic.", "skills": entries}),
    );
    write_json(
        &root,
        MARKETPLACE,
        &json!({
            "name": "synthetic",
            "owner": {"name": "Synthetic"},
            "plugins": [{"name": "synthetic-skills", "source": "./"}]
        }),
    );
    root
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    crate::dirs::private_dir_all(path.parent().expect("parent")).expect("dir");
    fs::write(path, text).expect("write");
}

fn write_json(root: &Path, rel: &str, value: &Value) {
    write(
        root,
        rel,
        &serde_json::to_string_pretty(value).expect("json"),
    );
}

fn stub(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Synthetic description of {name}.\n---\n\n{STUB}\n")
}

fn written(name: &str, notice: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Synthetic description of {name}.\n---\n\n\
         # Synthetic\n\nSynthetic steps.\n\n## Licence\n\n{notice}\n"
    )
}

const OPENAI: &str =
    "interface:\n  display_name: \"Synthetic\"\n  short_description: \"Synthetic skill\"\n";

fn notice() -> String {
    MATT_NOTICE.join("\n")
}

fn assert_clean(root: &Path) {
    let found = problems(root);
    assert!(
        found.is_empty(),
        "expected no problems, got:\n{}",
        found.join("\n")
    );
}

/// The check reports a problem containing `expected` for `root`.
fn assert_catches(root: &Path, expected: &str) {
    let found = problems(root);
    assert!(
        found.iter().any(|p| p.contains(expected)),
        "expected a problem containing {expected:?}, got:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_stubs_and_a_written_skill_pass() {
    let root = fixture("good");
    assert_clean(&root);
    write(&root, "skills/grill/SKILL.md", &written("grill", &notice()));
    write(&root, "skills/grill/agents/openai.yaml", OPENAI);
    assert_clean(&root);
    // Line breaks in the notice don't matter.
    write(
        &root,
        "skills/setup/SKILL.md",
        &written("setup", &collapse(&notice())),
    );
    write(&root, "skills/setup/agents/openai.yaml", OPENAI);
    assert_clean(&root);
    // Quoted values, as Matt's frontmatter uses them.
    write(
        &root,
        "skills/triage/SKILL.md",
        &format!("---\nname: \"triage\"\ndescription: 'Synthetic: it''s quoted.'\n---\n\n{STUB}\n"),
    );
    assert_clean(&root);
}

#[test]
fn a_missing_or_broken_frontmatter_fails() {
    let front = |lines: &str| format!("---\n{lines}\n---\n\n{STUB}\n");
    let long = "x".repeat(DESCRIPTION_MAX_CHARS + 1);
    let cases = [
        (
            "no-frontmatter",
            format!("# Synthetic\n\n{STUB}\n"),
            "must open with a `---` line",
        ),
        (
            "unclosed",
            "---\nname: grill\ndescription: Synthetic.\n".into(),
            "closed by a `---` line",
        ),
        (
            "no-description",
            front("name: grill"),
            "has no `description`",
        ),
        ("no-name", front("description: Synthetic."), "has no `name`"),
        (
            "wrong-name",
            front("name: grilling\ndescription: Synthetic."),
            "not its folder's name `grill`",
        ),
        (
            "folded",
            front("name: grill\ndescription: >\n  Synthetic."),
            "is not a one-line `key: value`",
        ),
        (
            "colon",
            front("name: grill\ndescription: Synthetic: broken."),
            "quote it",
        ),
        (
            "empty",
            front("name: grill\ndescription: \"\""),
            "`description` is empty",
        ),
        (
            "blank",
            front("name: grill\ndescription:"),
            "needs a value on the same line",
        ),
        (
            "no-space",
            front("name:grill\ndescription: Synthetic."),
            "needs a space after its `:`",
        ),
        (
            "boolean",
            front("name: grill\ndescription: true"),
            "reads as a YAML boolean",
        ),
        (
            "number",
            front("name: grill\ndescription: 1.5"),
            "reads as a YAML boolean",
        ),
        (
            "twice",
            front("name: grill\ndescription: One.\ndescription: Two."),
            "appears twice",
        ),
        (
            "long",
            front(&format!("name: grill\ndescription: {long}")),
            "longer than 1024",
        ),
    ];
    for (case, text, expected) in cases {
        let root = fixture(&format!("frontmatter-{case}"));
        write(&root, "skills/grill/SKILL.md", &text);
        assert_catches(&root, expected);
    }
}

#[test]
fn a_user_only_flag_fails() {
    let flagged =
        |flag: &str| format!("---\nname: grill\ndescription: Synthetic.\n{flag}\n---\n\n{STUB}\n");
    for (case, flag) in [
        ("claude", "disable-model-invocation: true"),
        ("claude-false", "disable-model-invocation: false"),
        ("hidden", "user-invocable: false"),
    ] {
        let root = fixture(&format!("user-only-{case}"));
        write(&root, "skills/grill/SKILL.md", &flagged(flag));
        assert_catches(
            &root,
            &format!("key `{}` is not allowed", flag.split(':').next().unwrap()),
        );
    }

    let root = fixture("user-only-codex");
    write(
        &root,
        "skills/grill/agents/openai.yaml",
        &format!("{OPENAI}policy:\n  allow_implicit_invocation: false\n"),
    );
    assert_catches(&root, "Codex's switch that makes a skill user-only");

    for (case, yaml, expected) in [
        (
            "no-interface",
            "  display_name: \"Synthetic\"\n",
            "must open with `interface:`",
        ),
        (
            "no-short",
            "interface:\n  display_name: \"Synthetic\"\n",
            "`interface.short_description` is missing",
        ),
        (
            "extra",
            &format!("{OPENAI}  icon_small: \"x.svg\"\n") as &str,
            "is not allowed",
        ),
    ] {
        let root = fixture(&format!("openai-{case}"));
        write(&root, "skills/grill/agents/openai.yaml", yaml);
        assert_catches(&root, expected);
    }
}

#[test]
fn a_missing_mit_notice_fails() {
    let root = fixture("notice-missing");
    write(
        &root,
        "skills/grill/SKILL.md",
        &written("grill", "Synthetic, no notice."),
    );
    write(&root, "skills/grill/agents/openai.yaml", OPENAI);
    assert_catches(&root, "Matt Pocock's copyright and MIT notice");

    let root = fixture("notice-changed");
    let changed = notice().replace("free of charge", "for a fee");
    write(&root, "skills/grill/SKILL.md", &written("grill", &changed));
    write(&root, "skills/grill/agents/openai.yaml", OPENAI);
    assert_catches(&root, "Matt Pocock's copyright and MIT notice");

    let root = fixture("notice-no-copyright");
    let unsigned = notice().replace("Copyright (c) 2026 Matt Pocock", "");
    write(&root, "skills/grill/SKILL.md", &written("grill", &unsigned));
    write(&root, "skills/grill/agents/openai.yaml", OPENAI);
    assert_catches(&root, "Matt Pocock's copyright and MIT notice");

    let root = fixture("written-no-openai");
    write(&root, "skills/grill/SKILL.md", &written("grill", &notice()));
    assert_catches(&root, "skills/grill/agents/openai.yaml is missing");

    let root = fixture("written-kept-stub-line");
    let kept = written("grill", &format!("{STUB_MARK} Synthetic.\n\n{}", notice()));
    write(&root, "skills/grill/SKILL.md", &kept);
    write(&root, "skills/grill/agents/openai.yaml", OPENAI);
    assert_catches(&root, "drops the stub line");
}

#[test]
fn a_dangling_or_wrong_link_fails() {
    let relink = |root: &Path, rel: &str, target: &str| {
        let link = root.join(rel);
        let _ = fs::remove_file(&link);
        symlink(target, link).expect("link");
    };

    let root = fixture("link-dangling");
    relink(&root, ".agents/skills/grill", "../../skills/gril");
    assert_catches(&root, ".agents/skills/grill points at `../../skills/gril`");
    assert_catches(&root, ".agents/skills/grill does not resolve");

    let root = fixture("link-missing");
    fs::remove_file(root.join(".agents/skills/setup")).expect("remove");
    assert_catches(
        &root,
        ".agents/skills/setup must be a link to `../../skills/setup`",
    );

    let root = fixture("link-other-skill");
    relink(&root, ".agents/skills/triage", "../../skills/setup");
    assert_catches(
        &root,
        ".agents/skills/triage points at `../../skills/setup`",
    );

    let root = fixture("link-absolute");
    let absolute = fs::canonicalize(root.join("skills/handoff")).expect("real");
    relink(
        &root,
        ".agents/skills/handoff",
        absolute.to_str().expect("utf-8"),
    );
    assert_catches(&root, ".agents/skills/handoff points at");

    let root = fixture("link-copied");
    fs::remove_file(root.join(".agents/skills/handoff")).expect("remove");
    write(&root, ".agents/skills/handoff/SKILL.md", &stub("handoff"));
    assert_catches(&root, ".agents/skills/handoff must be a link");

    let root = fixture("link-extra");
    relink(&root, ".agents/skills/old-skill", "../../skills/old-skill");
    assert_catches(
        &root,
        ".agents/skills/old-skill is a link, but not to one of the eight",
    );

    let root = fixture("link-extra-file");
    write(&root, ".agents/skills/notes.md", "Synthetic.\n");
    assert_catches(&root, ".agents/skills/notes.md: only the eight links");

    let root = fixture("link-inside-skills");
    relink(&root, "skills/grill/shared.md", "../README.md");
    assert_catches(&root, "skills/grill/shared.md is a link");

    let root = fixture("markdown-dangling");
    write(
        &root,
        "skills/README.md",
        "# Synthetic\n\nSee [gone](gone/SKILL.md).\n",
    );
    assert_catches(
        &root,
        "skills/README.md: link `gone/SKILL.md` does not resolve",
    );

    let root = fixture("markdown-outside");
    write(&root, "../outside-synthetic.md", "Synthetic.\n");
    write(
        &root,
        "skills/README.md",
        "# Synthetic\n\n[out]: ../../outside-synthetic.md\n",
    );
    assert_catches(&root, "leaves the repository");
}

#[test]
fn a_broken_manifest_fails() {
    let plugin = |skills: Value, extra: Value| {
        let mut value =
            json!({"name": "synthetic-skills", "description": "Synthetic.", "skills": skills});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        value
    };
    let all = || -> Vec<Value> {
        SKILLS
            .iter()
            .map(|n| json!(format!("./skills/{n}")))
            .collect()
    };

    let root = fixture("plugin-missing");
    fs::remove_file(root.join(PLUGIN)).expect("remove");
    assert_catches(&root, ".claude-plugin/plugin.json is missing");

    let root = fixture("plugin-not-json");
    write(&root, PLUGIN, "{ synthetic");
    assert_catches(&root, "is not JSON");

    let mut fewer = all();
    fewer.pop();
    let mut extra = all();
    extra.push(json!("./skills/extra"));
    let mut slash = all();
    slash[0] = json!("./skills/grill/");
    let mut twice = all();
    twice.push(json!("./skills/setup"));
    let cases = [
        (
            "unlisted",
            plugin(json!(fewer), json!({})),
            "does not list `./skills/triage`",
        ),
        (
            "extra",
            plugin(json!(extra), json!({})),
            "`./skills/extra` is not `./skills/<name>`",
        ),
        (
            "slash",
            plugin(json!(slash), json!({})),
            "`./skills/grill/` is not `./skills/<name>`",
        ),
        (
            "twice",
            plugin(json!(twice), json!({})),
            "`./skills/setup` is listed twice",
        ),
        (
            "hooks",
            plugin(json!(all()), json!({"hooks": "./hooks.json"})),
            "`hooks` is not allowed",
        ),
        (
            "version",
            plugin(json!(all()), json!({"version": "0.0.5"})),
            "`version` is not allowed",
        ),
        (
            "no-skills",
            json!({"name": "synthetic-skills", "description": "Synthetic."}),
            "`skills` must be an array",
        ),
    ];
    for (case, value, expected) in cases {
        let root = fixture(&format!("plugin-{case}"));
        write_json(&root, PLUGIN, &value);
        assert_catches(&root, expected);
    }

    let root = fixture("plugin-unresolved");
    fs::remove_file(root.join("skills/grill/SKILL.md")).expect("remove");
    assert_catches(&root, "`./skills/grill` does not resolve");

    let market = |plugins: Value| json!({"name": "synthetic", "owner": {"name": "Synthetic"}, "plugins": plugins});
    let cases = [
        (
            "source",
            market(json!([{"name": "synthetic-skills", "source": "./other"}])),
            "`source` must be `./`",
        ),
        (
            "name",
            market(json!([{"name": "other", "source": "./"}])),
            "`name` must be",
        ),
        (
            "two",
            market(
                json!([{"name": "synthetic-skills", "source": "./"}, {"name": "x", "source": "./"}]),
            ),
            "exactly one plugin entry",
        ),
        (
            "no-owner",
            json!({"name": "synthetic", "plugins": [{"name": "synthetic-skills", "source": "./"}]}),
            "`owner.name`",
        ),
    ];
    for (case, value, expected) in cases {
        let root = fixture(&format!("marketplace-{case}"));
        write_json(&root, MARKETPLACE, &value);
        assert_catches(&root, expected);
    }
}

#[test]
fn the_skill_set_is_exactly_the_eight() {
    let root = fixture("set-missing");
    fs::remove_dir_all(root.join("skills/setup")).expect("remove");
    assert_catches(&root, "skills/setup is missing");

    let root = fixture("set-extra");
    write(&root, "skills/extra/SKILL.md", &stub("extra"));
    assert_catches(
        &root,
        "skills/extra: only README.md and the eight skill folders",
    );

    let root = fixture("set-no-readme");
    fs::remove_file(root.join("skills/README.md")).expect("remove");
    assert_catches(&root, "skills/README.md is missing");
}
