//! The `openwiz-server` program: the CloudRoom adapter (`room-cli` from
//! `PATH`, as the `openwiz` CLI builds it) and the system clock, served on
//! 127.0.0.1.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use openwiz_adapter_cloudroom::source::CloudRoomSource;
use openwiz_server::{ALLOWED_HOST_ENV, Config, DEFAULT_PORT, NAME, SystemClock, WebRoot};

const EXIT_FAILED: u8 = 1;
const EXIT_USAGE: u8 = 2;

struct Args {
    port: u16,
    allowed_host: Option<String>,
    web_root: Option<PathBuf>,
}

enum Parsed {
    Run(Args),
    Help,
}

fn parse(args: &[String], env_host: Option<String>) -> Result<Parsed, String> {
    let (mut port, mut allowed_host, mut web_root) = (None, None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_owned())),
            _ => (arg.as_str(), None),
        };
        match flag {
            "--help" | "-h" => return Ok(Parsed::Help),
            "--port" | "--allowed-host" | "--web-root" => {
                let value = inline
                    .or_else(|| args.next().cloned())
                    .ok_or_else(|| format!("{flag} needs a value"))?;
                if flag == "--port" {
                    let n = value
                        .parse::<u16>()
                        .ok()
                        .filter(|_| value.bytes().all(|b| b.is_ascii_digit()))
                        .ok_or_else(|| format!("--port needs a number from 0 to {}", u16::MAX))?;
                    if port.replace(n).is_some() {
                        return Err("--port is given more than once".to_owned());
                    }
                } else if flag == "--web-root" {
                    if web_root.replace(PathBuf::from(value)).is_some() {
                        return Err("--web-root is given more than once".to_owned());
                    }
                } else if allowed_host.replace(value).is_some() {
                    return Err("--allowed-host is given more than once".to_owned());
                }
            }
            _ => return Err("unknown option".to_owned()),
        }
    }
    let allowed_host = allowed_host.or(env_host.filter(|h| !h.is_empty()));
    if let Some(host) = &allowed_host
        && !openwiz_server::valid_host_name(host)
    {
        return Err(
            "the allowed host must be a DNS name (letters, digits, - and .), without a port"
                .to_owned(),
        );
    }
    Ok(Parsed::Run(Args {
        port: port.unwrap_or(DEFAULT_PORT),
        allowed_host,
        web_root,
    }))
}

fn main() -> ExitCode {
    // A panic would print its message and source location; stderr carries
    // only the start line and the request log, so say only that it happened.
    std::panic::set_hook(Box::new(|_| eprintln!("{NAME}: internal error")));
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let env_host = std::env::var(ALLOWED_HOST_ENV).ok();
    let args = match parse(&args, env_host) {
        Ok(Parsed::Help) => {
            print!("{}", openwiz_server::help());
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Run(args)) => args,
        Err(message) => {
            eprintln!("{NAME}: {message} (see {NAME} --help)");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    // Loaded before listening. The path is the owner's: never echoed; the
    // refusal names only the rule.
    let web_root = match args.web_root.as_deref().map(WebRoot::open).transpose() {
        Ok(root) => root,
        Err(e) => {
            eprintln!(
                "{NAME}: cannot serve --web-root: {}",
                if e.kind() == std::io::ErrorKind::InvalidInput {
                    e.to_string()
                } else {
                    "it must be a directory with index.html".to_owned()
                }
            );
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let listener = match openwiz_server::bind(args.port) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("{NAME}: cannot listen on 127.0.0.1:{}: {e}", args.port);
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let page = if web_root.is_some() {
        ", with the web page"
    } else {
        ""
    };
    let config = Config {
        allowed_host: args.allowed_host,
        web_root,
        ..Config::default()
    };
    let server = match openwiz_server::start(
        listener,
        config,
        Arc::new(CloudRoomSource::from_path()),
        Arc::new(SystemClock),
        Arc::new(|line: &str| eprintln!("{NAME}: {line}")),
    ) {
        Ok(server) => server,
        Err(e) => {
            eprintln!("{NAME}: cannot start: {e}");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    eprintln!(
        "{NAME}: listening on http://{} (loopback only{page})",
        server.local_addr()
    );
    server.join();
    ExitCode::from(EXIT_FAILED)
}
