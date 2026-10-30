//! The web page: `apps/web`'s static export, from one directory given by
//! `--web-root` (A8). An explicit list that matches the export's layout:
//! `/` is `index.html`, and `/_next/static/…` and `/fonts/…` are files under
//! the root with an extension in [`TYPES`]. Nothing else is served.
//!
//! **Loaded once, at start, into memory; no request touches the
//! filesystem**. Replacing the root, an ancestor or a file
//! afterwards changes nothing, and a request can never open a FIFO. A new
//! export needs a restart.
//!
//! Fail-closed: the whole root is refused if any entry is a symlink, is not
//! a regular file or directory, has a name outside `[A-Za-z0-9._-]` or
//! starting with `.`, is a file with another extension, more than one link
//! or on another device, or passes a size cap. A request matches a loaded
//! path exactly; nothing is decoded. Every miss is the API's `404`.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;

use crate::http::Reply;

/// The page's Content-Security-Policy: the one the web page's own proofs
/// load it under (`apps/web/test/support/serve.mjs` `PAGE_CSP`). Scripts
/// and styles from this origin only, and no inline script or style.
pub const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
font-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; \
form-action 'none'; frame-ancestors 'none'";

/// `Cache-Control` for `/_next/static/…` and `/fonts/…`. A default, for now:
/// the user's own browser only, never a shared cache.
pub const STATIC_CACHE: &str = "private, max-age=31536000, immutable";

/// `Cache-Control` for the page itself: it names one build's assets.
const PAGE_CACHE: &str = "no-store";

/// The largest file served, in bytes. The export's largest is about 220 KB.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The most files loaded from the web root. The export has about 15.
pub const MAX_WEB_FILES: usize = 512;

/// The most bytes loaded from the web root in all. The export has about
/// 650 KB.
pub const MAX_WEB_BYTES: u64 = 8 * 1024 * 1024;

/// The web routes, as the log names them.
pub const WEB_ROUTES: &[&str] = &[PAGE, STATIC, FONTS];

const PAGE: &str = "/";
const STATIC: &str = "/_next/static/*";
const FONTS: &str = "/fonts/*";

/// The fixed content types, by extension. Any other extension is `404`.
const TYPES: &[(&str, &str)] = &[
    ("html", "text/html; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("css", "text/css; charset=utf-8"),
    ("woff2", "font/woff2"),
    ("txt", "text/plain; charset=utf-8"),
];

/// The headers on every web file, besides its type, cache and length.
pub(crate) const PAGE_HEADERS: &[(&str, &str)] = &[
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("X-Frame-Options", "DENY"),
    ("Content-Security-Policy", PAGE_CSP),
    ("Connection", "close"),
];

/// How deep directories may nest under the root.
const MAX_DEPTH: usize = 16;

/// One file, as loaded at start.
#[derive(Clone, PartialEq, Eq)]
struct Asset {
    content_type: &'static str,
    body: Vec<u8>,
}

/// The web page's files, loaded at start: request path (`/` for
/// `index.html`) to type and bytes. Immutable.
#[derive(Clone, PartialEq, Eq)]
pub struct WebRoot(Arc<BTreeMap<String, Asset>>);

impl fmt::Debug for WebRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebRoot({} files)", self.0.len())
    }
}

fn refused(why: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, why)
}

impl WebRoot {
    /// Loads `dir`, or refuses it whole (the error says which rule, never
    /// the path). A symlink in `dir` itself is resolved here, once.
    pub fn open(dir: &Path) -> io::Result<Self> {
        let root = fs::canonicalize(dir)?;
        let meta = fs::symlink_metadata(&root)?;
        if !meta.is_dir() {
            return Err(refused("not a directory"));
        }
        let mut loaded = Loaded {
            device: meta.dev(),
            files: BTreeMap::new(),
            bytes: 0,
        };
        loaded.directory(&root, "", 0)?;
        let mut files = loaded.files;
        let index = files
            .remove("index.html")
            .ok_or_else(|| refused("no regular index.html"))?;
        let mut served = BTreeMap::from([("/".to_owned(), index)]);
        for (relative, asset) in files {
            if relative.starts_with("_next/static/") || relative.starts_with("fonts/") {
                served.insert(format!("/{relative}"), asset);
            }
        }
        Ok(Self(Arc::new(served)))
    }
}

/// The walk at start: every entry under the root is checked, and every
/// file read.
struct Loaded {
    device: u64,
    files: BTreeMap<String, Asset>,
    bytes: u64,
}

impl Loaded {
    fn directory(&mut self, dir: &Path, prefix: &str, depth: usize) -> io::Result<()> {
        if depth > MAX_DEPTH {
            return Err(refused("directories nested too deep"));
        }
        for entry in fs::read_dir(dir)? {
            let name = entry?.file_name();
            let name = name
                .to_str()
                .filter(|n| segment(n))
                .ok_or_else(|| refused("a name outside [A-Za-z0-9._-], or starting with ."))?;
            let path = dir.join(name);
            let relative = format!("{prefix}{name}");
            let meta = fs::symlink_metadata(&path)?;
            let kind = meta.file_type();
            if kind.is_symlink() {
                return Err(refused("a symlink"));
            }
            if meta.dev() != self.device {
                return Err(refused("an entry on another device"));
            }
            if kind.is_dir() {
                self.directory(&path, &format!("{relative}/"), depth + 1)?;
                continue;
            }
            if !kind.is_file() {
                return Err(refused("an entry that is not a regular file or directory"));
            }
            let content_type =
                content_type(name).ok_or_else(|| refused("a file type outside the table"))?;
            if meta.nlink() != 1 {
                return Err(refused("a hard link"));
            }
            if meta.len() > MAX_FILE_BYTES {
                return Err(refused("a file over the size cap"));
            }
            if self.files.len() == MAX_WEB_FILES {
                return Err(refused("more files than the cap"));
            }
            let body = read(&path, &meta)?;
            self.bytes += body.len() as u64;
            if self.bytes > MAX_WEB_BYTES {
                return Err(refused("more bytes in all than the cap"));
            }
            self.files.insert(relative, Asset { content_type, body });
        }
        Ok(())
    }
}

/// Letters, digits, `.`, `_` and `-`; not empty; not starting with `.`.
fn segment(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn content_type(name: &str) -> Option<&'static str> {
    let (_, extension) = name.rsplit_once('.')?;
    TYPES.iter().find(|(e, _)| *e == extension).map(|(_, t)| *t)
}

/// Reads the file `checked` describes. This is the only open, at start:
/// its type was checked just before, so a FIFO swapped in between could
/// delay the start, never a request. The file opened must be the file
/// checked.
fn read(path: &Path, checked: &Metadata) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let opened = file.metadata()?;
    let same = (opened.dev(), opened.ino()) == (checked.dev(), checked.ino());
    if !opened.is_file() || !same || opened.nlink() != 1 {
        return Err(refused("a file that changed while loading"));
    }
    let mut body = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut body)?;
    if body.len() as u64 > MAX_FILE_BYTES {
        return Err(refused("a file over the size cap"));
    }
    Ok(body)
}

/// The web route template a path is under, if any.
pub(crate) fn route(path: &str) -> Option<&'static str> {
    if path == "/" {
        Some(PAGE)
    } else if path.starts_with("/_next/static/") {
        Some(STATIC)
    } else if path.starts_with("/fonts/") {
        Some(FONTS)
    } else {
        None
    }
}

/// The loaded file at `path` (under `template`), or `None` (`404`).
pub(crate) fn serve(root: &WebRoot, template: &str, path: &str) -> Option<Reply> {
    let asset = root.0.get(path)?;
    let cache = if template == PAGE {
        PAGE_CACHE
    } else {
        STATIC_CACHE
    };
    Some(Reply {
        status: 200,
        headers: PAGE_HEADERS,
        extra: vec![
            ("Content-Type", asset.content_type.to_owned()),
            ("Cache-Control", cache.to_owned()),
        ],
        body: asset.body.clone(),
    })
}
