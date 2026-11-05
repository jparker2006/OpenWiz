//! The `openwiz` program: the CloudRoom adapter (`room-cli` from `PATH`)
//! and the system clock, wired to [`openwiz_cli::run`]; for `send`, the
//! send record in the owner's state directory, wired to
//! [`openwiz_cli::send::run_send`]; for `new` and `models`, the
//! new-thread record beside it, wired to [`openwiz_cli::new_thread`]; and
//! for `attachment`, the attachment store beside them, wired to
//! [`openwiz_cli::attachment::run_attachment`] (no `room-cli`).

use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_cli::attachment::{self, AttachmentContext};
use openwiz_cli::new_thread::{self, NewContext};
use openwiz_cli::send::{self, SendContext};
use openwiz_cli::{EXIT_USAGE, SystemClock};
use openwiz_core::messaging::record::{SendStore, StoreError};
use openwiz_core::new_thread::record::NewThreadStore;
use openwiz_core::ops::Clock;
use openwiz_store::attachments::{Claims, Df, NoClaims, RecordedClaims};
use openwiz_store::{AttachmentStore, FileStore, NewThreadFileStore, default_state_dir};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = if args.first().is_some_and(|a| a == "send") {
        run_send(&args[1..])
    } else if args.first().is_some_and(|a| a == "new") {
        run_new(&args[1..])
    } else if args.first().is_some_and(|a| a == "attachment") {
        run_attachment(&args[1..])
    } else if args.first().is_some_and(|a| a == "models") {
        match utf8(&args[1..], "nothing was read") {
            Some(args) => new_thread::run_models(
                &args,
                &CloudRoomSource::from_path(),
                &SystemClock,
                &mut io::stdout().lock(),
                &mut io::stderr().lock(),
            ),
            None => EXIT_USAGE,
        }
    } else {
        let args: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        openwiz_cli::run(
            &args,
            &CloudRoomSource::from_path(),
            &SystemClock,
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// The arguments as UTF-8, or `None` after one `openwiz: ` line saying
/// `nothing` happened. A text must reach `room-cli` byte for byte, so an
/// argument that is not UTF-8 is refused rather than converted.
fn utf8(args: &[OsString], nothing: &str) -> Option<Vec<String>> {
    let args = args
        .iter()
        .map(|a| a.clone().into_string().ok())
        .collect::<Option<Vec<String>>>();
    if args.is_none() {
        let _ = writeln!(
            io::stderr(),
            "openwiz: an argument is not valid UTF-8; {nothing} (see openwiz --help)"
        );
    }
    args
}

/// `openwiz new`: the new-thread record in the owner's state directory.
fn run_new(args: &[OsString]) -> i32 {
    let Some(args) = utf8(args, "nothing was created") else {
        return EXIT_USAGE;
    };
    let open_store = || -> Result<Box<dyn NewThreadStore>, StoreError> {
        let dir = default_state_dir(&|name| std::env::var_os(name))?;
        Ok(Box::new(NewThreadFileStore::open(&dir)?))
    };
    let open_store_read_only = || -> Result<Option<Box<dyn NewThreadStore>>, StoreError> {
        let dir = default_state_dir(&|name| std::env::var_os(name))?;
        let store = NewThreadFileStore::open_read_only(&dir)?;
        Ok(store.map(|s| Box::new(s) as Box<dyn NewThreadStore>))
    };
    let open_attachments = || -> Result<AttachmentStore, StoreError> {
        let dir = default_state_dir(&|name| std::env::var_os(name))?;
        AttachmentStore::open(&dir)
    };
    let stdin = io::stdin();
    let stdin_is_terminal = stdin.is_terminal();
    new_thread::run_new_with(
        &args,
        NewContext {
            client: &CloudRoomSource::from_path(),
            open_store: &open_store,
            open_store_read_only: &open_store_read_only,
            clock: &SystemClock,
            stdin: &mut stdin.lock(),
            stdin_is_terminal,
            new_op_id: &send::random_op_id,
            wait: &std::thread::sleep,
        },
        &open_attachments,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}

/// `openwiz attachment`: the attachment store in the owner's state
/// directory, its free space measured by `/bin/df`. Paths are
/// passed as given, UTF-8 or not; nothing calls `room-cli`. Each
/// attachment's claim is read from its operation's record (B2): one a send
/// or a new thread may still use is never removed or expired.
fn run_attachment(args: &[OsString]) -> i32 {
    let open_store = || -> Result<AttachmentStore, StoreError> {
        let dir = default_state_dir(&|name| std::env::var_os(name))?;
        AttachmentStore::open(&dir)
    };
    let open_store_read_only = || -> Result<Option<AttachmentStore>, StoreError> {
        let dir = default_state_dir(&|name| std::env::var_os(name))?;
        AttachmentStore::open_read_only(&dir)
    };
    // Without a state directory the store can't be opened either, so the
    // claims are never asked.
    let claims: Box<dyn Claims> = match default_state_dir(&|name| std::env::var_os(name)) {
        Ok(dir) => Box::new(RecordedClaims::new(&dir, SystemClock.now())),
        Err(_) => Box::new(NoClaims),
    };
    attachment::run_attachment(
        args,
        AttachmentContext {
            open_store: &open_store,
            open_store_read_only: &open_store_read_only,
            clock: &SystemClock,
            free_space: &Df,
            claims: claims.as_ref(),
        },
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}

/// `openwiz send`. The text must reach `room-cli` byte for byte, so an
/// argument that is not UTF-8 is refused rather than converted.
fn run_send(args: &[OsString]) -> i32 {
    let Some(args) = utf8(args, "nothing was sent") else {
        return EXIT_USAGE;
    };
    let open_store = || -> Result<Box<dyn SendStore>, StoreError> {
        let dir = default_state_dir(&|name| std::env::var_os(name))?;
        Ok(Box::new(FileStore::open(&dir)?))
    };
    let open_store_read_only = || -> Result<Option<Box<dyn SendStore>>, StoreError> {
        let dir = default_state_dir(&|name| std::env::var_os(name))?;
        let store = FileStore::open_read_only(&dir)?;
        Ok(store.map(|s| Box::new(s) as Box<dyn SendStore>))
    };
    let open_attachments = || -> Result<AttachmentStore, StoreError> {
        let dir = default_state_dir(&|name| std::env::var_os(name))?;
        AttachmentStore::open(&dir)
    };
    let stdin = io::stdin();
    let stdin_is_terminal = stdin.is_terminal();
    send::run_send_with(
        &args,
        SendContext {
            messenger: &CloudRoomSource::from_path(),
            open_store: &open_store,
            open_store_read_only: &open_store_read_only,
            clock: &SystemClock,
            stdin: &mut stdin.lock(),
            stdin_is_terminal,
            new_op_id: &send::random_op_id,
        },
        &open_attachments,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}
