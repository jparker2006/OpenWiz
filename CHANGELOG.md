# Changelog

All OpenWiz versions, newest first. Versioning: [RELEASING.md](RELEASING.md). What's tested and what isn't: [docs/testing.md](docs/testing.md).

## 0.0.5 · 2026-11-05

**Fleets of agents build for you**

- Eight fleet skills for coding agents: grill you on an idea, plan it into tickets, then build, review and merge each one.
- They ship as a Claude Code plugin, and as skill folders Codex reads from the repo.
- Every merge needs an independent review of the exact commit and green CI.

## 0.0.4 · 2026-11-05

**Start new threads, with files**

- `openwiz new` starts a Cloud thread in one of your projects. It shows exactly what it will do, and only creates it with `--yes`.
- Attach files and images to a new thread or a queued message, from the terminal or the web page.
- One thread per create: if the result is unclear, it tells you instead of trying again.

## 0.0.3 · 2026-11-05

**Message your threads**

- `openwiz send` sends one message to a thread, as Queue or Steer.
- Or send from the web page on your phone.
- If a send's result is unclear, OpenWiz says so instead of sending twice.

## 0.0.2 · 2026-10-30

**Your threads on your phone**

- A private web page served from your own computer: threads by project, with status and age; tap one for details.
- Reach it from anywhere through a Cloudflare Tunnel and a login for your email only.
- Read-only, and it tells you when data is stale or you're signed out.

## 0.0.1 · 2026-10-30

**Your CloudRoom threads in the terminal**

- `openwiz ls` lists your threads with status, project and age; `-l` adds a recap, model and effort.
- `openwiz show <id>` shows one thread and its final output.
- `--json` for scripts. Read-only: it never changes a thread.

## 0.0.0 · 2026-10-22

**The design**

- Published first as OpenFleet: the design, architecture and release rules, no code yet. Renamed to OpenWiz before 0.0.1.
