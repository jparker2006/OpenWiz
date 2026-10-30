//! The `openwiz` program: the CloudRoom adapter (`room-cli` from `PATH`)
//! and the system clock, wired to [`openwiz_cli::run`].

use std::io;
use std::process::ExitCode;

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_cli::SystemClock;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let code = openwiz_cli::run(
        &args,
        &CloudRoomSource::from_path(),
        &SystemClock,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
