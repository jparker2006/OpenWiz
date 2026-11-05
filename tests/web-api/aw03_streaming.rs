use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use openwiz_server::{Limits, MAX_NAME_BYTES};

use crate::attach_support::{
    ATTACHMENTS, Free, State, encode, ready_id, serve_attach, upload_headers, upload_with,
};
use crate::send_support::{raw, with};

#[test]
fn an_upload_streams_into_the_store_and_writes_nothing_beside_it() {
    let state = State::new("web_aw3_stream");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let mut headers = upload_headers("aw3-stream.txt");
    headers.push(("Content-Length".to_owned(), "1000000".to_owned()));
    let mut stream = TcpStream::connect(s.addr).expect("connect");
    stream
        .write_all(&raw("POST", ATTACHMENTS, &headers, b"", false))
        .unwrap();
    stream.write_all(&vec![b's'; 70_000]).unwrap();
    s.wait_for("the upload to hold its slot", |st| st.uploads == 1);
    // The bytes are already the store's unfinished copy.
    let until = Instant::now() + Duration::from_secs(10);
    while state.partials().is_empty() {
        assert!(
            Instant::now() < until,
            "no .partial in the store while the body arrives: {:?}",
            listing(&state.dir)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // Nothing beside the store: the state directory holds only its area.
    assert_eq!(
        listing(&state.dir),
        ["attachments"],
        "a file beside the store"
    );
    // Cut off, it leaves nothing.
    drop(stream);
    s.wait_for("the upload to end", |st| {
        st.connections == 0 && st.uploads == 0
    });
    assert!(state.nothing_staged(), "{:?}", state.entries());
    assert_eq!(listing(&state.dir), ["attachments"]);
}

/// The names in `dir`, sorted; none if it doesn't exist.
fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_name_up_to_the_cap_is_staged_whole() {
    assert_eq!(MAX_NAME_BYTES, 1024, "the header's cap, not a file name's");
    let state = State::new("web_aw3_long_name");
    let s = serve_attach(&state, Free::plenty(), Limits::default());
    let good = upload_headers("aw3.txt");
    let name = format!("{}.txt", "n".repeat(MAX_NAME_BYTES - 4));
    let r = upload_with(
        &s,
        &with(&good, "X-OpenWiz-Name", &encode(&name)),
        b"Synthetic AW3\n",
    );
    ready_id(&r);
    let staged = &r.json()["ready"][0]["staged"]["attachment"];
    assert_eq!(
        staged["display_name"],
        name.as_str(),
        "the owner's name, whole"
    );
    assert_eq!(
        staged["name"].as_str().map(str::len),
        Some(179),
        "the core's safe name"
    );
}

const STORE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../crates/store/src/attachments.rs"
);
const CLI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../crates/cli/src/attachment.rs"
);
const SERVER_SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/server/src");

/// The reports' types (their fields: the CLI's text wraps them, as
/// `struct AddReport(…)`, never redefines them) and their one presentation
/// rule each (`expires_at`, the broken-copy words, which removals were
/// expirations).
const REPORT_DEFINITIONS: &[&str] = &[
    "struct StagedView",
    "struct AddReport {",
    "struct AddReady",
    "struct AddFailed",
    "struct NotStaged",
    "struct ListReport {",
    "struct Broken",
    "enum RemoveResult",
    "struct RemoveEntry",
    "struct RemoveReport {",
    "fn broken_message",
    "fn claim_word",
    "UNSENT_TTL",
];

#[test]
fn the_reports_have_one_implementation_in_the_store() {
    let store = fs::read_to_string(STORE).expect("the store");
    assert!(
        store.contains("pub mod report"),
        "the store's report module"
    );
    for definition in REPORT_DEFINITIONS {
        if *definition == "UNSENT_TTL" {
            assert!(
                store.contains(definition),
                "the store's expires_at uses {definition}"
            );
        } else {
            assert_eq!(
                store.matches(definition).count(),
                1,
                "the store defines {definition} once"
            );
        }
    }
    let mut clients = vec![(CLI.to_owned(), fs::read_to_string(CLI).expect("the CLI"))];
    for entry in fs::read_dir(SERVER_SRC).expect("server sources") {
        let path = entry.expect("entry").path();
        clients.push((
            path.display().to_string(),
            fs::read_to_string(&path).expect("source"),
        ));
    }
    for (path, text) in &clients {
        for definition in REPORT_DEFINITIONS {
            assert!(
                !text.contains(definition),
                "{path} defines {definition}: a second implementation"
            );
        }
    }
    let uses = |end: &str| {
        clients
            .iter()
            .find(|(p, _)| p.ends_with(end))
            .is_some_and(|(_, t)| t.contains("attachments::report"))
    };
    assert!(
        uses("cli/src/attachment.rs"),
        "the CLI calls the store's reports"
    );
    assert!(
        uses("server/src/attachments.rs"),
        "the server calls the store's reports"
    );
}

/// A body that gives `first` bytes, then waits for the test: the rest, or
/// an end (the client gone).
struct Held {
    first: Vec<u8>,
    rest: std::sync::mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
}

impl std::io::Read for Held {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.buf.is_empty() {
            if !self.first.is_empty() {
                self.buf = std::mem::take(&mut self.first);
            } else {
                match self.rest.recv() {
                    Ok(more) => self.buf = more,
                    Err(_) => return Ok(0),
                }
            }
        }
        let n = out.len().min(self.buf.len());
        out[..n].copy_from_slice(&self.buf[..n]);
        self.buf.drain(..n);
        Ok(n)
    }
}

#[test]
fn two_stagings_at_once_never_go_below_the_floor() {
    use openwiz_core::attachment::{FREE_DISK_FLOOR_BYTES, FailReason};
    use openwiz_store::AttachmentStore;
    use openwiz_store::attachments::Staged;

    const LEN: u64 = 1024 * 1024;
    let state = State::new("web_aw3_floor_race");
    // Room for one and a half uploads above the floor: each fits alone.
    let free = Free(std::sync::atomic::AtomicU64::new(
        FREE_DISK_FLOOR_BYTES + LEN * 3 / 2,
    ));
    let now = crate::new_support::clock().0;
    let (tx, rx) = std::sync::mpsc::channel();
    let first = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            let store = AttachmentStore::open(&state.dir).unwrap();
            let mut body = Held {
                first: vec![b'a'; 8192],
                rest: rx,
                buf: vec![],
            };
            store.stage_stream("aw3-first.txt", LEN, &mut body, now, &free)
        });
        // The first is copying: its unfinished copy is in the store.
        let until = Instant::now() + Duration::from_secs(10);
        while state.partials().is_empty() {
            assert!(Instant::now() < until, "the first never started copying");
            std::thread::sleep(Duration::from_millis(5));
        }
        let store = AttachmentStore::open(&state.dir).unwrap();
        // A second stream, whole and ready to go: refused.
        let mut whole: &[u8] = &vec![b'b'; usize::try_from(LEN).unwrap()];
        let second = store
            .stage_stream("aw3-second.txt", LEN, &mut whole, now, &free)
            .unwrap();
        assert_eq!(
            second,
            Staged::Failed(FailReason::LowDisk),
            "a stream crossed the floor"
        );
        // A file, as `openwiz attachment add` stages it: refused too.
        let sources = state.dir.with_file_name("web_aw3_floor_race-sources");
        crate::dirs::private_dir_all(&sources).unwrap();
        let path = sources.join("aw3-file.txt");
        fs::write(&path, vec![b'c'; usize::try_from(LEN).unwrap()]).unwrap();
        let third = store.stage(&path, now, &free).unwrap();
        assert_eq!(
            third,
            Staged::Failed(FailReason::LowDisk),
            "a file crossed the floor"
        );
        let _ = fs::remove_dir_all(&sources);
        // The first's client goes away: it fails, and its reservation goes.
        drop(tx);
        a.join().unwrap()
    });
    assert_eq!(first.unwrap(), Staged::Failed(FailReason::ConnectionLost));
    assert!(state.nothing_staged(), "{:?}", state.entries());
    // Now the second fits.
    let store = AttachmentStore::open(&state.dir).unwrap();
    let mut whole: &[u8] = &vec![b'b'; usize::try_from(LEN).unwrap()];
    let again = store
        .stage_stream("aw3-second.txt", LEN, &mut whole, now, &free)
        .unwrap();
    assert!(matches!(again, Staged::Ready(_)), "{again:?}");
}

/// The bytes of every file under `dir`: a simulated disk's usage.
fn used(dir: &Path) -> u64 {
    let mut total = 0;
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            total += used(&path);
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}

/// A free-space measure over a simulated disk of `base` bytes, whose usage
/// is the store's own files. With a `hook`, the measure runs it after
/// taking its snapshot and before returning: another staging's copy moves
/// on in between .
struct Simulated<'a> {
    base: u64,
    hook: std::sync::Mutex<Option<Box<dyn FnOnce() + Send + 'a>>>,
}

impl openwiz_store::attachments::FreeSpace for Simulated<'_> {
    fn free_bytes(&self, dir: &Path) -> Result<u64, String> {
        let snapshot = self.base.saturating_sub(used(dir));
        if let Some(hook) = self.hook.lock().unwrap().take() {
            hook();
        }
        Ok(snapshot)
    }
}

#[test]
fn a_copy_moving_on_while_another_measures_never_crosses_the_floor() {
    moving_copy(false);
}

/// The same, with the first copy finishing in between: it is then counted
/// twice (still needed, and already used), never not at all.
#[test]
fn a_copy_finishing_while_another_measures_never_crosses_the_floor() {
    moving_copy(true);
}

fn moving_copy(finishes: bool) {
    use openwiz_core::attachment::{FREE_DISK_FLOOR_BYTES, FailReason};
    use openwiz_store::AttachmentStore;
    use openwiz_store::attachments::{META, Staged};

    const LEN: u64 = 1024 * 1024;
    const HEAD: usize = 8192;
    let base = FREE_DISK_FLOOR_BYTES + LEN * 3 / 2;
    {
        let state = State::new(&format!("web_aw3_floor_moving_{finishes}"));
        let attachments = state.dir.join("attachments");
        let now = crate::new_support::clock().0;
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let more = tx.clone();
        let first_dir = || {
            state.entries().into_iter().find(|d| {
                fs::read_dir(d)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|e| e.file_name().to_string_lossy().starts_with("aw3-first"))
            })
        };
        let written = |d: &Path| -> u64 {
            fs::read_dir(d)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("aw3-first"))
                .map(|e| e.metadata().map_or(0, |m| m.len()))
                .sum()
        };
        let (first, second) = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                let store = AttachmentStore::open(&state.dir).unwrap();
                let mut body = Held {
                    first: vec![b'a'; HEAD],
                    rest: rx,
                    buf: vec![],
                };
                let plain = Simulated {
                    base,
                    hook: std::sync::Mutex::new(None),
                };
                store.stage_stream("aw3-first.txt", LEN, &mut body, now, &plain)
            });
            let until = Instant::now() + Duration::from_secs(10);
            while first_dir().is_none_or(|d| written(&d) < HEAD as u64) {
                assert!(Instant::now() < until, "the first never started copying");
                std::thread::sleep(Duration::from_millis(5));
            }
            // While the second measures: the first moves on by three
            // quarters of its length, or to its end, and the measure waits.
            let hook = move || {
                let step = if finishes {
                    LEN as usize - HEAD
                } else {
                    LEN as usize * 3 / 4
                };
                more.send(vec![b'a'; step]).unwrap();
                let until = Instant::now() + Duration::from_secs(10);
                loop {
                    let dir = first_dir().expect("the first's directory");
                    let done = if finishes {
                        dir.join(META).exists()
                    } else {
                        written(&dir) >= (HEAD + step) as u64
                    };
                    if done {
                        break;
                    }
                    assert!(Instant::now() < until, "the first never moved on");
                    std::thread::sleep(Duration::from_millis(5));
                }
            };
            let measure = Simulated {
                base,
                hook: std::sync::Mutex::new(Some(Box::new(hook))),
            };
            let store = AttachmentStore::open(&state.dir).unwrap();
            let mut whole: &[u8] = &vec![b'b'; usize::try_from(LEN).unwrap()];
            let second = store.stage_stream("aw3-second.txt", LEN, &mut whole, now, &measure);
            // The first may finish now.
            let _ = tx.send(vec![b'a'; LEN as usize]);
            drop(tx);
            (a.join().unwrap(), second)
        });
        assert!(
            matches!(first, Ok(Staged::Ready(_))),
            "{finishes}: {first:?}"
        );
        assert_eq!(
            second.unwrap(),
            Staged::Failed(FailReason::LowDisk),
            "{finishes}: the second was let in, though both don't fit"
        );
        assert!(
            base - used(&attachments) >= FREE_DISK_FLOOR_BYTES,
            "{finishes}: the simulated disk ended under the floor"
        );
    }
}
