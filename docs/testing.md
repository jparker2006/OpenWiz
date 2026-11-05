# Testing and gaps for 0.0.3–0.0.5

**Current scope, 2026-11-05.** This update combines send, new-thread and attachment code with eight fleet skills. It does not add a fleet-state service or a fleet screen.

| Claim | Evidence class | Limit |
| --- | --- | --- |
| Inventory, send, new-thread, attachment and HTTP behaviour follows the encoded cases | Fixture test | Synthetic inputs and fake room-cli; not proof of real delivery or creation |
| Web composer, launcher and attachment tray follow their encoded cases | Fixture test | Linux Chromium; not Safari/WebKit or physical mobile devices |
| Sent and Cloud queued receipt parsing includes individually observed shapes | Fixture test plus limited live observation of upstream shape | Parser cases are synthetic reconstructions; receipt parsing does not establish full release acceptance |
| 0.0.3 live delivery acceptance | **Live observation: release-blocking claims held; gaps recorded** | LIVE-0.0.3: PC sittings on earlier 0.0.3 candidates, ending on `d4b2a82`; see the bounded result below. Not a live run of the combined export |
| 0.0.4 live create/attachment acceptance | **Pending live test** | LIVE-0.0.4: orchestrator fills in exact checked commit, environment, result, counts, gaps and a public-safe summary before claiming acceptance |
| Eight skills were used in a development run | Live observation, limited | Accepted with gaps; automatic discovery in fresh children unobserved, path-based briefs used |
| Reviews named exact heads and merged matching heads | Live observation, limited | Eight review reports and four matching merges; no low-finding follow-up exercised |
| Helper and structural checks exercise the skill rules | Fixture test | Historical helper results and the current export check are distinct; no claim that every helper was re-run for this export |

The skill run recorded ten starts, no duplicate builders started by the loop, and at most three ready tickets launched together. Five simultaneous ready tickets were not exercised. Cross-spec starts were observed; a start triggered by a merge from the preceding product version was not. The loop found seven hand-backs but needed intervention when earlier agents could not be reached; one later reviewer recovery was observed. Contract holds were fixture-tested, not observed live. Dependency names written in prose could evade path-based holds. Some cleanup required intervention, and API-budget exhaustion paused the loop. User intervention was still needed, so unattended operation is unverified.

**Evidence method:** the fleet summary is a read-back of recorded approvals, start claims, review reports, merge heads and recovery/cleanup records from the 2026-10-31 development run, through 23:30 UTC. Records posted from a shared account do not identify their author unless they say so. The accepted gaps remain limits of that run. Receipt-shape observations are also bounded upstream observations, not a successful complete live check of this export.

**Tested inputs:** fixtures use the historical room-cli 0.43.1 and v127 directories; catalogue shape observations were recorded with v131, and the Cloud queued receipt shape with v141. Folder names do not assert compatibility. The 0.0.3 PC test used room-cli 0.43.1 and v141 across sittings; 0.0.4's final live result/environment remains a placeholder.

**Fresh-clone verification:** the private export record binds build, test, formatting and Clippy logs to an exact public-tree head. These checks read no real CloudRoom account. The Rust and web toolchains and locked dependency versions are listed in [dependencies](dependencies.md).

## LIVE-0.0.3: recorded PC result

**Live observation, with gaps (2026-11-05).** The delivery test finished with the release-blocking claims held: each intended text reached its target exactly once without changing the judged cold sessions; tested lost acknowledgements stayed uncertain without automatic resend; the tested crash paths did not redispatch; operation-id conflicts were refused and the races made exactly one call; six server owner/request probes were refused, the public probe redirected to login, and those probes made no tells. This is a bounded result, not a guarantee for every environment or failure.

**Checked code and environment:** CLI sittings used `01edadf` and `86cda50`; the later web and phone steps used `d4b2a82`. These are the recorded 0.0.3 candidates, not this combined 0.0.5 export. The host was a Linux PC; room-cli was 0.43.1 initially and v141 later. One send ran on a real iPhone in Safari over cellular. The final agent-run steps used headless Chrome 157: an emulated iPhone for the phone steps and desktop Chromium for the dash-text send, through a local TLS pass-through rather than the tunnel/Access path.

**Counts and method:** the 23-send allowance was exhausted. Receipt inspection recorded 12 sent and two queued receipts with the observed key sets. Idle-send and Steer parity matched by comparing reports across sittings; busy-Queue parity was not comparable because the Local and Cloud targets/heads differed. The final read-safety check passed in a quiet window with 48 valid inventory, 48 session and 48 preview responses and zero tell attempts. Two earlier read-safety attempts failed because other fleet agents started threads during their windows; the quiet-window run is the passing result.

**Fault and device limits:** the final emulated phone step showed the pre-press effect line, then still reported sent after CDP went offline: offline emulation did not interrupt the in-flight request. The intended network-cut fault was not produced. The settled follow-up made zero calls. The dash text and file-path text arrived unchanged. These observations do not establish that a message joined an already running turn.

Remaining gaps:

- Queue on busy Cloud after the receipt fix was not re-observed live.
- The unsupported-mode case could not be produced with room-cli 0.43.1 or v141; the revised expectation was not rerun.
- The network-cut fault was not produced by emulation.
- Safari/WebKit beyond the one real phone send, broader real-radio checks and Blink on a physical phone are unverified.
- The tunnel and Access path was not exercised for the final emulated phone steps.
- No second identity was tested; the signed-in cross-site browser probe was skipped.
- Missing event labels leave whether a message joined the running turn unknown.
- Busy-Queue CLI/web parity remains incomparable in the recorded sittings.

**Machine-neutral effects:** this export takes the corrected environment-copy and awake-until-confirmed wording from the release line. Both refer to “this computer”; effect kinds and contract shapes remain unchanged. The source-wording reconciliation is complete. This wording correction is fixture-tested and does not itself establish 0.0.4 live acceptance.

**Release slots:** LIVE-0.0.3 is filled with the result and limits above; LIVE-0.0.4 remains **pending live test**. Still open: the quick read-only Mac check for this release; exact-head independent export review; maintainer approval of files, diff, date and commit email. Safari/WebKit, physical device radios and the recorded fleet gaps remain unproved. Headless Chromium device emulation must be described as emulation. No PACP conformance is claimed.
