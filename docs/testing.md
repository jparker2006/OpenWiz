# Testing and gaps for 0.0.3–0.0.5

**Current scope, 2026-11-05.** This update combines send, new-thread and attachment code with eight fleet skills. It does not add a fleet-state service or a fleet screen.

| Claim | Evidence class | Limit |
| --- | --- | --- |
| Inventory, send, new-thread, attachment and HTTP behaviour follows the encoded cases | Fixture test | Synthetic inputs and fake room-cli; not proof of real delivery or creation |
| Web composer, launcher and attachment tray follow their encoded cases | Fixture test | Linux Chromium; not Safari/WebKit or physical mobile devices |
| Sent and Cloud queued receipt parsing includes individually observed shapes | Fixture test plus limited live observation of upstream shape | Parser cases are synthetic reconstructions; receipt parsing does not establish full release acceptance |
| 0.0.3 live delivery acceptance | **Pending live test** | LIVE-0.0.3: orchestrator fills in exact checked commit, environment, result, counts, gaps and a public-safe summary before claiming acceptance |
| 0.0.4 live create/attachment acceptance | **Pending live test** | LIVE-0.0.4: orchestrator fills in exact checked commit, environment, result, counts, gaps and a public-safe summary before claiming acceptance |
| Eight skills were used in a development run | Live observation, limited | Accepted with gaps; automatic discovery in fresh children unobserved, path-based briefs used |
| Reviews named exact heads and merged matching heads | Live observation, limited | Eight review reports and four matching merges; no low-finding follow-up exercised |
| Helper and structural checks exercise the skill rules | Fixture test | Historical helper results and the current export check are distinct; no claim that every helper was re-run for this export |

The skill run recorded ten starts, no duplicate builders started by the loop, and at most three ready tickets launched together. Five simultaneous ready tickets were not exercised. Cross-spec starts were observed; a start triggered by a merge from the preceding product version was not. The loop found seven hand-backs but needed intervention when earlier agents could not be reached; one later reviewer recovery was observed. Contract holds were fixture-tested, not observed live. Dependency names written in prose could evade path-based holds. Some cleanup required intervention, and API-budget exhaustion paused the loop. User intervention was still needed, so unattended operation is unverified.

**Evidence method:** the fleet summary is a read-back of recorded approvals, start claims, review reports, merge heads and recovery/cleanup records from the 2026-10-31 development run, through 23:30 UTC. Records posted from a shared account do not identify their author unless they say so. The accepted gaps remain limits of that run. Receipt-shape observations are also bounded upstream observations, not a successful complete live check of this export.

**Tested inputs:** fixtures use the historical room-cli 0.43.1 and v127 directories; catalogue shape observations were recorded with v131, and the Cloud queued receipt shape with v141. Folder names do not assert compatibility. Full live version environments for 0.0.3/0.0.4 remain placeholders.

**Fresh-clone verification:** the private export record binds build, test, formatting and Clippy logs to an exact public-tree head. These checks read no real CloudRoom account. The Rust and web toolchains and locked dependency versions are listed in [dependencies](dependencies.md).

**Unverified effect wording:** this release line retains Mac-specific new-thread effects text, including the Mac staying awake until start confirmation and copying project environment files from the Mac. Those are source-documented expectations, not an observation on the PC. A live result for another checked code line must be reconciled with this exact source before claiming acceptance.

**Open release placeholders:** LIVE-0.0.3; LIVE-0.0.4; the quick read-only Mac check for this release; exact-head independent export review; maintainer approval of files, diff, date and commit email. Safari/WebKit, physical device radios and the recorded fleet gaps remain unproved. Headless Chromium device emulation must be described as emulation. No PACP conformance is claimed.
