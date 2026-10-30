# Releasing

## Version lines

Each line has its own numbers. A number on one line never implies anything about another.

| Line | Current | Where it is recorded |
| --- | --- | --- |
| **OpenWiz** (the product) | `0.0.2`: the read-only inventory, as a command (0.0.1) and as a private web page (0.0.2). *Updated for 0.0.2* | `CHANGELOG.md`, and the Rust workspace version in `Cargo.toml`. *Updated for 0.0.1:* the CLI does not print its own version or commit yet. *Updated for 0.0.2:* the server reports its version on `/api/v1/health`, and `apps/web/package.json` carries the same version |
| **PACP** (the protocol) | `v0.1` (draft) | [PACP versioning](docs/pacp/v0.1.md#13-versioning) and its version history |
| **Adapters** (one per personal agent) | none exist | each adapter's own notes, when one exists |

OpenWiz is developed in a private beta before each public release. Beta builds carry pre-release versions, such as `0.0.1-beta.1`. Only a release reviewed through the process below appears here.

## Versions are not dependencies

A version number says when something changed. It does not say what works with what. Real technical dependencies are stated explicitly for each release that contains software:

- the PACP version or versions it implements;
- the external tools and services it was tested against, with exact versions, such as a CloudRoom CLI version;
- the minimum toolchain needed to build it, such as the Rust and Node.js versions.

If a dependency is not listed with evidence, it is not claimed. A higher version on one line never upgrades a claim on another.

## What 0.0.0 is

0.0.0 is a set of proposal documents. It contains no implementation, package, install step, endpoint or test suite. Its release notes and any tag must say **"design preview, no implementation"**. A 0.0.0 release does not mean that any component works or that PACP has been implemented anywhere.

**Revising 0.0.0.** The 0.0.0 documents may be revised to correct them or to record later design decisions, such as the rename to OpenWiz. Each revision:
- is listed in `CHANGELOG.md` with its date;
- keeps the version at 0.0.0;
- never adds software or a capability claim.

The first version with software will be 0.0.1. Its planned first job is a read-only inventory of real CloudRoom sessions (see [decisions](docs/decisions.md#001-a-read-only-inventory)). Before it is tagged, it must state:
- how to install it;
- what was tested, and on what evidence;
- its explicit dependencies.

*Updated for 0.0.1:* 0.0.1 states how to install it in the [README](README.md#install), and what was tested, on what evidence, and what is not proven in [docs/evidence/0.0.1-live-check.md](docs/evidence/0.0.1-live-check.md), linked from the [changelog](CHANGELOG.md), and its dependencies in [docs/dependencies.md](docs/dependencies.md). Evidence that holds private data, such as a live check of the maintainer's own sessions, is published as its method, rules and counts, without the private data: [docs/evidence/0.0.1-live-check.md](docs/evidence/0.0.1-live-check.md). *Updated for 0.0.2:* 0.0.2 does the same in [docs/evidence/0.0.2-live-check.md](docs/evidence/0.0.2-live-check.md), and its access setup is published with placeholders only ([docs/web-access.md](docs/web-access.md)).

## How a release is reviewed

Work is prepared privately and published here only through an allowlisted export review:

1. **Allowlist.** Only files named on the release's approved file list are published. Directories are not copied wholesale.
2. **Written for the public.** Public files are written fresh with synthetic examples. Private notes, history and records are never copied in.
3. **Privacy and secret check.** Every file is read in full and scanned for personal data, credentials, private links and private identifiers.
4. **Claim check.** Every statement about capability has a status label and is no stronger than its evidence.
5. **Maintainer approval.** The maintainer approves the exact file list and diff.
6. **Publish and verify.** The change is published as an ordinary commit without force-pushing. The remote tree is then compared with the approved files.

The release does not happen if any step fails. Tags and GitHub Releases are created only by the maintainer, and only after step 6.

## What a version contains

- **A version has at most five meaningful outcomes, preferably one to three.** An outcome is a concrete result a user can see or check, such as "a read-only inventory of real sessions". It is not an activity, such as "improvements".
- **No vague bullets.** A large feature bundle never hides under a vague bullet. If a version would need more than five outcomes, it is split into more versions.
- **Every outcome is proof-driven.** Each one names its claims and the evidence each claim needs before work starts, and reports that evidence at release. Tests alone do not prove live behaviour, and nothing is called formally proven without an actual formal proof. See [docs/workflow.md](docs/workflow.md#proof-driven-development).

## Changelog

`CHANGELOG.md` lists every version and every revision of a version, newest first. *Updated for 0.0.1:* each entry has:
- the version and its date;
- a short headline, saying what the version is for;
- a short numbered list of user-facing changes: one plain sentence each, what a user can now do or see, usually five to seven items.

**A change is not an outcome.** The outcomes (at most five, above) are what a version sets out to prove; the changelog lists what a user gets. Every item belongs to one of the version's outcomes, and no item may claim more than that outcome's evidence shows. For software, what was tested and what remains unproven is in `docs/evidence/`, linked from the entry, not in the changelog itself. Entries before 0.0.1 use the older form, with outcomes listed directly.

Entries are written for the public when the change is published. They never copy private notes.

## What a release never claims

- that it works with any specific personal agent, vendor or service, the pilot targets included, unless that has been tested and the evidence is published;
- that any controller command or button exists before it has shipped;
- that anything conforms to PACP, because no conformance suite exists yet.

Every release keeps the unmodified [Apache License 2.0](LICENSE) text and any third-party attribution its contents require.
