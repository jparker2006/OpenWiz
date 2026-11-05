# Fleet skills

**Current files; limited live observation.** Eight skills describe a workflow: grill, plan-graph, launch-fleet, build-ticket, review-ticket, handoff, setup and triage. A fleet is the agents assigned to one approved spec; a ticket owns a declared set of paths. These are instructions and shell helpers, not a fleet daemon.

Run `setup` to record your own project settings in `docs/agents/`. The templates are in [skills/setup](../skills/setup/SKILL.md). Record approval of each plan before launching. Each builder owns one ticket and opens one pull request. A separate reviewer reads its exact head. Merges require the project's recorded rules and a read-back from GitHub. Brief agents by the skill's file path when discovery is unavailable.

**Live limits:** the development run was accepted with gaps; see [testing](testing.md). Neither the five-minute cadence, automatic discovery, complete contract holds nor unattended cleanup is guaranteed. The helper may exhaust a shared API budget. Do not infer that a skill instruction was successfully exercised merely because it is written down.
