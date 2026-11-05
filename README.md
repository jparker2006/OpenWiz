# OpenWiz

OpenWiz lets you see and message your CloudRoom threads, start Cloud threads with attachments, and coordinate work with fleet skills.

## What works today

- **0.0.1:** List and inspect your CloudRoom threads from the terminal.
- **0.0.2:** View your threads in a private web page on your phone or computer.
- **0.0.3:** Queue a message or steer a thread from the terminal or web page.
- **0.0.4:** Start a Cloud thread in an existing project and attach files or images to new threads or queued messages.
- **0.0.5:** Use eight fleet skills to plan, build and review work with coding agents.

## Get started

For the CLI, you need CloudRoom with `room-cli` on `PATH` and signed in, plus Rust 1.99.1.

From this checkout:

```sh
cargo build --release --locked
./target/release/openwiz --help
./target/release/openwiz ls
```

[Changelog](CHANGELOG.md) · [Docs](docs/) · [Agent and contributor guide](AGENTS.md)

[Apache 2.0](LICENSE).
