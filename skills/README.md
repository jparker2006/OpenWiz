# Fleet skills

**Current files; fixture-tested structure.** The eight folders are `grill`, `plan-graph`, `launch-fleet`, `build-ticket`, `review-ticket`, `handoff`, `setup` and `triage`. Each holds its `SKILL.md`, a Codex display file and Matt Pocock's MIT notice. See [fleet workflow](../docs/fleets.md), [glossary](../GLOSSARY.md) and [testing limits](../docs/testing.md).

**Use:** tell an agent `Read skills/<name>/SKILL.md and follow it`. Run `setup` first in the destination project. The plugin manifest in `.claude-plugin/` lists exactly the eight folders. `.agents/skills/` contains repository aliases. No Claude project configuration is shipped in this export; install through your harness's plugin mechanism or read the files directly.

**Live observation, limited:** fresh CloudRoom children did not discover repository skills automatically; file-path briefs were used. Plugin installation across every harness remains unverified. The skill bodies describe a workflow and requirements, not a claim of universal compatibility or unattended success.
