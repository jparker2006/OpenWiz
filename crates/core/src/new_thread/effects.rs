//! What a create does, worked out before creating, in the core's words,
//! and the project rules it starts from (`standard` projects with
//! a GitHub remote).
//!
//! These are the documented effects of a Cloud create, not
//! observations: the live check observes them, or records them as
//! unobserved.

use serde::Serialize;

use crate::model::{Fact, Timestamp};

use super::outcome::RefusalReason;
use super::{ProjectRef, ProjectView};

/// One effect of a create.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    CreatesAndStarts,
    StartsFromGithub,
    CopiesEnvFiles,
    FullPermissions,
    UsesCloudAgent,
    MacStaysAwake,
    CloudRoomNamesIt,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Effect {
    pub kind: EffectKind,
    pub text: String,
}

/// The effects of one create, from the project read at `read_at`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Effects {
    pub read_at: Timestamp,
    pub project: ProjectView,
    pub effects: Vec<Effect>,
}

/// The effects of creating in `project`, read at `read_at`. A GitHub remote
/// is required, so the start is always from GitHub.
pub fn effects_for(project: &ProjectView, read_at: Timestamp) -> Effects {
    let name = match &project.name {
        Fact::Known(name) => name.clone(),
        Fact::Unknown(_) => project.id.clone(),
    };
    let effect = |kind: EffectKind, text: String| Effect { kind, text };
    Effects {
        read_at,
        project: project.clone(),
        effects: vec![
            effect(
                EffectKind::CreatesAndStarts,
                format!("creates one Cloud thread in {name} and starts its first turn"),
            ),
            effect(
                EffectKind::StartsFromGithub,
                "starts from the project's default branch on GitHub".to_owned(),
            ),
            effect(
                EffectKind::CopiesEnvFiles,
                "copies the project's .env files from this computer".to_owned(),
            ),
            effect(
                EffectKind::FullPermissions,
                "runs with full permissions in its own sandbox".to_owned(),
            ),
            effect(
                EffectKind::UsesCloudAgent,
                "uses one Cloud agent from your plan".to_owned(),
            ),
            effect(
                EffectKind::MacStaysAwake,
                "this computer must stay awake until the start is confirmed".to_owned(),
            ),
            effect(
                EffectKind::CloudRoomNamesIt,
                "CloudRoom names it".to_owned(),
            ),
        ],
    }
}

/// One project as the adapter read it from `project list`, for a create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectCandidate {
    pub id: String,
    pub name: Fact<String>,
    /// `standard`, `personal`, or anything else as reported.
    pub kind: Fact<String>,
    /// Whether its remote is a GitHub remote.
    pub github_remote: Fact<bool>,
}

/// The names `candidate` answers to: its name, and the inventory's
/// qualified `name~<last 6 characters of the id>`.
fn display_names(candidate: &ProjectCandidate) -> Vec<String> {
    let Fact::Known(name) = &candidate.name else {
        return vec![];
    };
    let tail: String = {
        let chars: Vec<char> = candidate.id.chars().collect();
        chars[chars.len().saturating_sub(6)..].iter().collect()
    };
    vec![name.clone(), format!("{name}~{tail}")]
}

/// The project `reference` names among `projects` (the default project
/// list: hidden projects are not in it), if OpenWiz may create there.
/// Otherwise why not. Nothing is created in an unknown, hidden, personal or
/// non-standard project, or one without a GitHub remote.
///
/// A name matches every project that answers to it, by its literal name or
/// its qualified one, and more than one project is ambiguous: a literal
/// name never hides another project's qualified name.
pub fn resolve_project(
    reference: &ProjectRef,
    projects: &[ProjectCandidate],
) -> Result<ProjectView, RefusalReason> {
    let mut found: Vec<&ProjectCandidate> = projects
        .iter()
        .filter(|p| match reference {
            ProjectRef::Id(id) => &p.id == id,
            ProjectRef::Name(name) => display_names(p).contains(name),
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found.dedup_by(|a, b| a.id == b.id);
    let project = match found.as_slice() {
        [] => return Err(RefusalReason::ProjectNotFound),
        [one] => *one,
        _ => return Err(RefusalReason::ProjectAmbiguous),
    };
    match &project.kind {
        Fact::Known(kind) if kind == "standard" => {}
        Fact::Known(kind) if kind == "personal" => return Err(RefusalReason::ProjectPersonal),
        _ => return Err(RefusalReason::ProjectNotStandard),
    }
    match project.github_remote {
        Fact::Known(true) => {}
        Fact::Known(false) => return Err(RefusalReason::ProjectNoGithubRemote),
        Fact::Unknown(_) => return Err(RefusalReason::ProjectRemoteNotConfirmed),
    }
    Ok(ProjectView {
        id: project.id.clone(),
        name: project.name.clone(),
    })
}
