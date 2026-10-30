# Test fixtures (synthetic only)

**Every value here is invented.** No real session content, identifier, title, path, email or URL is in this folder. `cargo test -p openwiz-tests --test checks fixture_scan` enforces this on every run.

## Where the shapes come from

The **keys, types, nullability, enum labels and exit codes** follow real `room-cli` output, observed by the maintainer on their own machine. Only structure was recorded: never a value, an id, a title, session content or a path. The observations are not published, because the raw output holds private session data, so treat these shapes as reported, not independently checkable.

There were four observations. These fixtures keep them apart:

| Capture | What it gives these fixtures |
| --- | --- |
| **C** (2026-10-23, `room-cli` 0.43.1, across all rows) | The `thread list` row shape, including what is optional and what is null on cloud rows; `project list`, `--hidden` (`[]`) and `machine list`; the cloud `thread show` shape (`environment: null`); both `thread output` shapes (`error` null, and `error` a string with exit 0); not-found (exit 1, empty stdout, `Error: HTTP 404: Thread not found`) |
| **A** (2026-10-23, 0.43.1, mostly first-row types) | `--version` as plain text; agreement on the project and machine shapes; that every `projectId` resolved and every local row's host was a listed machine |
| **B** (2026-10-22, version **not recorded**) | Only the local `thread show` shape: the `environment` object, and that `purpose` can be absent |
| **V** (2026-10-28, `room-cli` v127) | The `--version` text `v127` (`v<N>`, one line). It found no incompatible change to the shapes OpenWiz reads, so `normal` prints `v127` and answers everything else with the 0.43.1 shapes |

## Synthetic assumptions

Anything the captures did not observe is a **synthetic assumption**. Each scenario lists its own in `synthetic_assumptions`. They are:

1. **Timestamps are milliseconds** since the Unix epoch. Only their type (int) was recorded.
2. **Hidden sessions.** `thr_syn_0007` has `visibility: "hidden"` and appears only with `--include-hidden`. No hidden session existed to observe.
3. **`project list --json --include-personal --hidden`** returns `[]`. C observed `[]` for `--hidden`; combining it with `--include-personal` is assumed.
4. **`thread show` of an archived session** (`thr_syn_0008`) and of a **`claude-code` cloud session** (`thr_syn_0006`) have the observed shapes. Neither was observed.
5. **No final output yet** (`thr_syn_0005`) is an empty `output` string. It was not observed.
6. **Values inside the local `environment` object** that were only typed (`status`, `lifecycle.phase`, `environmentProviderInstanceKey`, `workspaceProvisionType`, `pullRequest.status`) use `syn-` labels.
7. **Failures other than not-found**: the HTTP 500 line, the hung read, not-JSON, truncated, wrong-type and missing-field responses are all invented.
8. **Unknown extra keys** (`syntheticExtraField`, …) stand for fields a later `room-cli` might add.
9. **`0.44.0`** is an invented version number.
10. **Unresolved references.** `thr_syn_0003`'s project `prj_syn_0099` is not in the project list, and `thr_syn_0004`'s host `mch_syn_0099` is not in the machine list. Capture A saw every project id resolve and every local host listed, so both cases are invented.
11. **Both flags at once.** `thread list --json --include-hidden --include-archived` is assumed to return the union of the two lists. It was not observed.
12. **A project disappearing between runs.** The `project-removed` scenario drops `prj_syn_0001` from the project list while its sessions still name it. P6 reads one source with `normal`, then `project-removed`, then `normal` again, to catch grouping kept from an earlier run. Unobserved.

**Not covered by any fixture:** read-side effects. Nothing here shows that a read cannot wake a session or mark it read. 0.0.1's live check looked for such effects; see the changelog for what it covered.

## Layout

| Path | Contents |
| --- | --- |
| `room-cli-0.43.1/<scenario>/scenario.json` | What the fake `room-cli` answers for each allowlisted argument list. A scenario may `extends` another |
| `room-cli-0.43.1/normal/` | The normal case: 6 default sessions, plus one hidden and one archived, 5 projects (two share a name), 1 machine |
| `room-cli-0.43.1/<variant>/` | Command error and timeout (of `thread list`, `project list`, `thread show` or `thread output`), not JSON, truncated, wrong types, missing fields, unknown extra fields, versions 0.44.0 and 0.43.1 (both untested), and a project removed between runs (`project-removed`). The nonexistent id `thr_syn_9999` is in `normal` |
| `envelopes/` | Example `openwiz.inventory.v1` envelopes, checked against `contracts/inventory.v1.schema.json` |

## The synthetic world

| Session | Where | Status | Project | Notes |
| --- | --- | --- | --- | --- |
| `thr_syn_0001` | cloud | idle | `prj_syn_0001` | Top-level (`parentThreadId: null`) |
| `thr_syn_0002` | local, `mch_syn_0001` | active | `prj_syn_0002` | Purpose over 120 characters; final output over 4 KiB, with a 2-byte character across byte 4096 |
| `thr_syn_0003` | cloud | error | `prj_syn_0099`, **not listed** | Final output with a reported error |
| `thr_syn_0004` | local, `mch_syn_0099`, **not listed** | idle | `prj_syn_0003` (personal) | |
| `thr_syn_0005` | cloud | idle | `prj_syn_0004` | Child of `thr_syn_0001`; no final output yet |
| `thr_syn_0006` | cloud | active | `prj_syn_0005` | Same project name as `prj_syn_0004` |
| `thr_syn_0007` | cloud | idle | `prj_syn_0001` | Hidden (synthetic assumption) |
| `thr_syn_0008` | cloud | idle | `prj_syn_0002` | Archived; no `purpose` key |

Row `thr_syn_000N` was updated N minutes before the tests' fixed clock (`1700000600000`).
