//! W15 : the built `openwiz-server` dispatches nothing when it starts,
//! and a server killed during a send's call leaves that op id uncertain:
//! a replay after the restart is recovered as uncertain, never sent again.
//! And U9: the server and `openwiz send` share one send record, so an op id
//! claimed by one is replayed by the other, never sent twice.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::http::{self, Response};
use crate::msg::{self, SendFake, json_of};
use crate::send_support::{HOST, body, good_headers, messages, raw};

/// The built server, over `fake`'s `room-cli` and send record.
struct Binary {
    child: Child,
    addr: SocketAddr,
    stderr: Arc<Mutex<String>>,
    /// Appends each stderr line to `stderr` as it arrives, until the server
    /// ends.
    reader: Option<thread::JoinHandle<()>>,
}

impl Binary {
    fn start(fake: &SendFake) -> Self {
        let mut child = Command::new(http::server_binary())
            .args(["--port", "0", "--allowed-host", HOST])
            .env("PATH", fake.path())
            .env("OPENWIZ_STATE_DIR", &fake.state)
            .current_dir(&fake.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start openwiz-server");
        let mut stderr = BufReader::new(child.stderr.take().unwrap());
        let mut first = String::new();
        stderr.read_line(&mut first).unwrap();
        let addr = first
            .trim()
            .strip_prefix("openwiz-server: listening on http://")
            .and_then(|rest| rest.split(' ').next())
            .and_then(|a| a.parse().ok())
            .unwrap_or_else(|| panic!("unexpected first line {first:?}"));
        let log = Arc::new(Mutex::new(first));
        let sink = log.clone();
        let reader = thread::spawn(move || {
            for line in stderr.lines() {
                let Ok(line) = line else { break };
                let mut log = sink.lock().unwrap();
                log.push_str(&line);
                log.push('\n');
            }
        });
        Self {
            child,
            addr,
            stderr: log,
            reader: Some(reader),
        }
    }

    /// Stops the server, then returns all it wrote to stderr. Stopping it
    /// can cut off a line not yet written: wait for the lines first
    /// ([`Binary::wait_for_lines`]).
    fn stop(&mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            reader.join().expect("the stderr reader");
        }
        self.stderr.lock().unwrap().clone()
    }

    /// Waits, up to 10 s, until stderr holds `n` lines starting with each
    /// `prefix`: a line is written after its response is sent, so a response
    /// does not prove its line is there yet .
    fn wait_for_lines(&self, expected: &[(&str, usize)]) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let log = self.stderr.lock().unwrap().clone();
            if expected.iter().all(|(prefix, n)| count(&log, prefix) >= *n) {
                return;
            }
            assert!(Instant::now() < until, "the log lines never came:\n{log}");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn post(&self, value: &Value) -> Response {
        http::send(
            self.addr,
            &raw(
                "POST",
                &messages("thr_syn_0001"),
                &good_headers(),
                value.to_string().as_bytes(),
                true,
            ),
        )
    }

    fn lookup(&self, op: &str) -> Response {
        http::send(
            self.addr,
            &raw(
                "GET",
                &format!("/api/v1/sessions/thr_syn_0001/messages/{op}"),
                &[("Host".to_owned(), HOST.to_owned())],
                b"",
                false,
            ),
        )
    }
}

/// A log line's start: the lookup, and a send, each answered `200`.
const LOOKUP_LINE: &str = "openwiz-server: GET /api/v1/sessions/{id}/messages/{op_id} 200 ";
const POST_LINE: &str = "openwiz-server: POST /api/v1/sessions/{id}/messages 200 ";

/// The lines of `log` that start with `prefix`.
fn count(log: &str, prefix: &str) -> usize {
    log.lines().filter(|line| line.starts_with(prefix)).count()
}

impl Drop for Binary {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_server_killed_during_the_call_never_sends_again() {
    let fake = SendFake::with(
        "web_w15_kill",
        vec![msg::tell_response(
            "thr_syn_0001",
            "queue",
            json!({ "kill_parent": true }),
        )],
    );
    let value = body("w15-op-1", "queue", "Synthetic W15 text");
    let mut first = Binary::start(&fake);
    // The fake kills the server during its call: no answer comes.
    let mut stream = TcpStream::connect(first.addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    stream
        .write_all(&raw(
            "POST",
            &messages("thr_syn_0001"),
            &good_headers(),
            value.to_string().as_bytes(),
            true,
        ))
        .unwrap();
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    assert!(
        !String::from_utf8_lossy(&answer).starts_with("HTTP/1.1 200"),
        "the killed server answered"
    );
    // Never wait forever: a server that was not killed fails the proof.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = first.child.try_wait().expect("the server's state") {
            break status;
        }
        assert!(
            std::time::Instant::now() < until,
            "the server was not killed by its call"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success(), "{status:?}");
    assert_eq!(fake.tell_count(), 1);
    assert_eq!(fake.record("w15-op-1")["state"], json!("dispatching"));

    // A new server: its start makes no call at all.
    let calls = fake.invocations().len();
    let mut second = Binary::start(&fake);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(fake.invocations().len(), calls, "the start called room-cli");
    // The lookup sees it as uncertain, and changes nothing.
    let r = second.lookup("w15-op-1");
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["result"]["report"]["status"], json!("uncertain"));
    assert_eq!(fake.record("w15-op-1")["state"], json!("dispatching"));
    // A replay of the same op id: recovered as uncertain, not sent again.
    let r = second.post(&value);
    assert_eq!(r.status, 200, "{}", r.text());
    let report = r.json();
    assert_eq!(report["status"], json!("uncertain"), "{report}");
    assert_eq!(report["result"]["recovered"], json!(true), "{report}");
    let r = second.post(&value);
    assert_eq!(r.json()["status"], json!("uncertain"));
    assert_eq!(fake.tell_count(), 1, "the op id was sent again");
    // Every line first, then stop the server: stopping it could cut one off.
    second.wait_for_lines(&[(LOOKUP_LINE, 1), (POST_LINE, 2)]);
    let log = second.stop();
    // The log holds exactly this server's start line and its three
    // requests, before its privacy is checked.
    assert_eq!(count(&log, LOOKUP_LINE), 1, "the log is incomplete:\n{log}");
    assert_eq!(count(&log, POST_LINE), 2, "the log is incomplete:\n{log}");
    assert_eq!(log.lines().count(), 4, "the log lines:\n{log}");
    assert!(!log.contains("Synthetic W15"), "{log}");
    assert!(!log.contains("w15-op-1"), "{log}");
    assert!(!log.contains("thr_syn_0001"), "{log}");
}

#[test]
fn the_server_and_the_cli_share_one_send_record() {
    let fake = SendFake::new("web_u9_shared", "send");
    let server = Binary::start(&fake);
    let a = body("u9-op-a", "queue", "Synthetic U9 text a");
    let r = server.post(&a);
    assert_eq!(r.status, 200, "{}", r.text());
    assert_eq!(r.json()["status"], json!("accepted"));
    // The CLI, with the same environment, replays it.
    let out = fake.run_process(
        &[
            "send",
            "--op-id",
            "u9-op-a",
            "--json",
            "--",
            "thr_syn_0001",
            "Synthetic U9 text a",
        ],
        "",
    );
    let (code, report) = json_of(&out);
    assert_eq!(code, Some(0));
    assert_eq!(report["result"]["replayed"], json!(true), "{report}");
    assert_eq!(fake.tell_count(), 1);
    // And the other way round.
    let out = fake.run_process(
        &[
            "send",
            "--op-id",
            "u9-op-b",
            "--json",
            "--",
            "thr_syn_0001",
            "Synthetic U9 text b",
        ],
        "",
    );
    assert_eq!(json_of(&out).0, Some(0));
    assert_eq!(fake.tell_count(), 2);
    let r = server.post(&body("u9-op-b", "queue", "Synthetic U9 text b"));
    assert_eq!(r.json()["result"]["replayed"], json!(true), "{}", r.text());
    let r = server.lookup("u9-op-b");
    assert_eq!(
        r.json()["result"]["kind"],
        json!("recorded"),
        "{}",
        r.text()
    );
    assert_eq!(fake.tell_count(), 2);
}
