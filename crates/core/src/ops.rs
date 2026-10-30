//! Operations: `list` (for `ls` and `ls -l`) and `show`.
//!
//! Every run reads the source's version first and reports it; the version
//! is never a gate. Then `list` reads the session, project and machine lists, and
//! `show` reads one session, the project and machine lists, and its final
//! output. Grouping and location use only the lists read in the same run.
//!
//! The order is the sort key: most recently updated first, then by session
//! id; sessions whose update time is unknown come last, by session id.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{self, AtomicUsize};
use std::thread;
use std::time::{Duration, Instant};

use crate::envelope::{
    Completeness, Counts, Coverage, Envelope, Freshness, Gap, GapKind, SCHEMA_ID, SourceInfo,
};
use crate::limits::{
    DEFAULT_LIMIT, FIELD_MAX_CHARS, FINAL_OUTPUT_MAX_BYTES, MAX_CONCURRENT_DETAIL_READS, MAX_LIMIT,
    PURPOSE_MAX_CHARS, RUN_BUDGET, cap_field,
};
use crate::model::{
    BoundedText, Fact, FinalOutput, Grouping, Inventory, Location, ProjectId, Recap,
    ReportedConfiguration, Session, SessionId, SessionView, Timestamp,
};
use crate::source::{
    MachineRecord, Placement, ProjectRecord, Read, Rows, SessionDetailRecord, SessionRecord,
    SessionScope, SessionSource, SourceError,
};

/// Reads the time. The core takes all time from here, never from the
/// system directly, so snapshots, ages and deadlines can be tested.
pub trait Clock: Send + Sync {
    /// Wall-clock time, for the snapshot and row ages.
    fn now(&self) -> Timestamp;

    /// Monotonic time, for the run budget and every read's deadline.
    fn monotonic(&self) -> Instant;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// `ls`: no detail reads.
    Short,
    /// `ls -l`: detail reads for the requested page only.
    Long,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRequest {
    pub detail: Detail,
    /// `--hidden`.
    pub include_hidden: bool,
    /// `-a`.
    pub include_archived: bool,
    /// At most [`MAX_LIMIT`]; a larger limit is refused.
    pub limit: usize,
    pub offset: usize,
    pub budget: Duration,
}

impl Default for ListRequest {
    fn default() -> Self {
        Self {
            detail: Detail::Short,
            include_hidden: false,
            include_archived: false,
            limit: DEFAULT_LIMIT,
            offset: 0,
            budget: RUN_BUDGET,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShowRequest {
    pub id: SessionId,
    pub budget: Duration,
}

/// An explicit error: no honest partial result is possible. Every string in
/// it is within [`FIELD_MAX_CHARS`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpError {
    /// The session list itself could not be read.
    ListFailed(SourceError),
    /// `show` of a session the source does not know. Nothing more is read.
    NotFound(SessionId),
    /// `show` could not read the session at all.
    ShowFailed(SourceError),
    /// The request itself is out of bounds, such as a limit above the cap or
    /// an id the source cannot accept.
    InvalidRequest(String),
}

/// What the run read as the source's version. Never a gate:
/// any version runs; one other than the live-tested version is marked
/// untested, and text that is not a recognised version is unknown, with a gap.
#[derive(Clone, Debug, PartialEq, Eq)]
struct VersionRead {
    version: Fact<String>,
    tested: bool,
    gap: Option<Gap>,
}

/// A failed version read, described by safe facts only: the exit code or the
/// timeout, never the source's own text, which could hold private detail.
fn version_failure(error: &SourceError) -> String {
    match error {
        SourceError::CommandFailed {
            exit_code: Some(code),
            ..
        } => format!("exited {code} (detail withheld)"),
        SourceError::CommandFailed {
            exit_code: None, ..
        } => "ended without an exit code (detail withheld)".to_owned(),
        SourceError::TimedOut { after_ms } => format!("timed out after {after_ms} ms"),
        SourceError::Malformed { .. } => "returned malformed output (detail withheld)".to_owned(),
        SourceError::Unavailable { .. } => "could not be run (detail withheld)".to_owned(),
        SourceError::NotFound { .. } | SourceError::InvalidId { .. } => {
            "failed (detail withheld)".to_owned()
        }
    }
}

/// A recognised version: `N.N.N` (1 to 4 digits each), as `room-cli` 0.43.1
/// printed, or `v<N>` (1 to 6 digits), as `room-cli` v127 prints.
fn is_version_text(text: &str) -> bool {
    let digits = |s: &str, max: usize| {
        !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit())
    };
    if let Some(n) = text.strip_prefix('v') {
        return digits(n, 6);
    }
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| digits(p, 4))
}

/// A result and what its display needs beyond the envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report<T> {
    pub envelope: Envelope<T>,
    /// Projects in the result whose name another project in the same run's
    /// project list shares. Their display name is `name~<last 6 characters
    /// of the id>`; the JSON keeps the reported name.
    pub shared_names: BTreeSet<ProjectId>,
}

/// The reason on a configuration that `ls` without `-l` does not read.
pub const NOT_READ_SHORT: &str = "not read: ls without -l";

/// The reason on anything the run budget left unread.
pub const NOT_READ_BUDGET: &str = "not read: budget";

/// `ls` and `ls -l`.
pub fn list(
    source: &dyn SessionSource,
    clock: &dyn Clock,
    request: &ListRequest,
) -> Result<Envelope<Inventory>, OpError> {
    list_report(source, clock, request).map(|r| r.envelope)
}

/// `show <id>`.
pub fn show(
    source: &dyn SessionSource,
    clock: &dyn Clock,
    request: &ShowRequest,
) -> Result<Envelope<SessionView>, OpError> {
    show_report(source, clock, request).map(|r| r.envelope)
}

/// [`list`], with the shared project names for display.
pub fn list_report(
    source: &dyn SessionSource,
    clock: &dyn Clock,
    request: &ListRequest,
) -> Result<Report<Inventory>, OpError> {
    if request.limit > MAX_LIMIT {
        return Err(OpError::InvalidRequest(format!(
            "--limit {} is above the cap of {MAX_LIMIT}",
            request.limit
        )));
    }
    let run = Run::start(source, clock, request.budget);
    let version = run.read_version();
    let scope = SessionScope {
        include_hidden: request.include_hidden,
        include_archived: request.include_archived,
    };
    let rows = source
        .list_sessions(scope, run.deadline)
        .map_err(|e| OpError::ListFailed(cap_error(e)))?;

    let mut gaps = Vec::new();
    let listed = rows.records.len() + rows.unreadable.len();
    let mut unreadable = rows.unreadable.len();
    gaps.extend(rows.unreadable);
    let mut seen = BTreeSet::new();
    let mut kept = Vec::new();
    for record in rows.records {
        if record.id.0.is_empty() || record.id.0.chars().count() > FIELD_MAX_CHARS {
            unreadable += 1;
            gaps.push(gap(
                GapKind::Malformed,
                None,
                format!(
                    "{} returned a session id that is empty or longer than {FIELD_MAX_CHARS} characters",
                    source.read_name(Read::Sessions)
                ),
            ));
        } else if !seen.insert(record.id.clone()) {
            unreadable += 1;
            gaps.push(gap(
                GapKind::Malformed,
                Some(record.id.0.clone()),
                format!(
                    "{} returned this session id more than once; it is shown once",
                    source.read_name(Read::Sessions)
                ),
            ));
        } else {
            gaps.extend(record.problems.iter().cloned());
            kept.push(record);
        }
    }

    let (mut excluded_hidden, mut excluded_archived) = (0, 0);
    let mut matched = Vec::new();
    for record in kept {
        if record.hidden == Fact::Known(true) && !request.include_hidden {
            excluded_hidden += 1;
        } else if record.archived == Fact::Known(true) && !request.include_archived {
            excluded_archived += 1;
        } else {
            matched.push(record);
        }
    }
    matched.sort_by(sort_key);
    let matched_count = matched.len();
    let page: Vec<SessionRecord> = matched
        .into_iter()
        .skip(request.offset)
        .take(request.limit)
        .collect();

    let projects = run.projects(request.include_hidden, &mut gaps);
    let machines = run.machines(&mut gaps);

    let configurations: Vec<ReportedConfiguration> = match request.detail {
        Detail::Short => page
            .iter()
            .map(|_| not_read(NOT_READ_SHORT.to_owned()))
            .collect(),
        Detail::Long => {
            let ids: Vec<SessionId> = page.iter().map(|r| r.id.clone()).collect();
            run.details(&ids)
                .into_iter()
                .zip(&ids)
                .map(|(outcome, id)| run.configuration(id, outcome, &mut gaps))
                .collect()
        }
    };
    let sessions: Vec<Session> = page
        .iter()
        .zip(configurations)
        .map(|(record, configuration)| {
            build_session(record, configuration, &projects, &machines, run.snapshot)
        })
        .collect();
    let shared_names = shared_names(&projects, sessions.iter().map(|s| &s.group));
    let counts = Counts {
        listed: listed as u64,
        unreadable: unreadable as u64,
        excluded_hidden,
        excluded_archived,
        hidden_read: request.include_hidden,
        archived_read: request.include_archived,
        matched: matched_count as u64,
        offset: request.offset as u64,
        limit: request.limit as u64,
        shown: sessions.len() as u64,
    };
    Ok(Report {
        envelope: run.envelope(version, Some(counts), gaps, Inventory { sessions }),
        shared_names,
    })
}

/// [`show`], with the shared project names for display.
pub fn show_report(
    source: &dyn SessionSource,
    clock: &dyn Clock,
    request: &ShowRequest,
) -> Result<Report<SessionView>, OpError> {
    let run = Run::start(source, clock, request.budget);
    let version = run.read_version();
    let detail = source
        .session_detail(&request.id, run.deadline)
        .map_err(|e| match e {
            SourceError::NotFound { .. } => OpError::NotFound(request.id.clone()),
            SourceError::InvalidId { id, detail } => OpError::InvalidRequest(cap_field(&format!(
                "{id} cannot be passed to {}: {detail}",
                source.name()
            ))),
            other => OpError::ShowFailed(cap_error(other)),
        })?;
    let mut gaps: Vec<Gap> = detail.session.problems.clone();
    let projects = run.projects(false, &mut gaps);
    let machines = run.machines(&mut gaps);
    let (final_output, reported_error) = run.output(&detail.session.id, &mut gaps);
    let configuration = ReportedConfiguration {
        model: cap_fact(&detail.model),
        effort: cap_fact(&detail.effort),
    };
    let session = build_session(
        &detail.session,
        configuration,
        &projects,
        &machines,
        run.snapshot,
    );
    let shared_names = shared_names(&projects, [&session.group]);
    let view = SessionView {
        session,
        final_output,
        reported_error,
    };
    Ok(Report {
        envelope: run.envelope(version, None, gaps, view),
        shared_names,
    })
}

/// The sort key: most recently updated first, then by id; unknown update
/// times last, by id.
fn sort_key(a: &SessionRecord, b: &SessionRecord) -> Ordering {
    let by_time = match (&a.updated_at, &b.updated_at) {
        (Fact::Known(x), Fact::Known(y)) => y.cmp(x),
        (Fact::Known(_), Fact::Unknown(_)) => Ordering::Less,
        (Fact::Unknown(_), Fact::Known(_)) => Ordering::Greater,
        (Fact::Unknown(_), Fact::Unknown(_)) => Ordering::Equal,
    };
    by_time.then_with(|| a.id.cmp(&b.id))
}

/// A list that was read in this run, or why it was not.
type Listed<T> = Result<T, String>;

/// One run: its deadline, snapshot time and source.
struct Run<'a> {
    source: &'a dyn SessionSource,
    clock: &'a dyn Clock,
    deadline: Instant,
    snapshot: Timestamp,
}

/// What became of one detail read.
enum Outcome {
    Read(Box<SessionDetailRecord>),
    Failed(SourceError),
    NotRead,
}

impl<'a> Run<'a> {
    fn start(source: &'a dyn SessionSource, clock: &'a dyn Clock, budget: Duration) -> Self {
        let deadline = clock.monotonic() + budget;
        Self {
            source,
            clock,
            deadline,
            snapshot: clock.now(),
        }
    }

    fn out_of_budget(&self) -> bool {
        self.clock.monotonic() >= self.deadline
    }

    fn name(&self, read: Read) -> &str {
        self.source.read_name(read)
    }

    /// `<read> <what went wrong>`, such as `thread show exited 1: …`.
    fn describe(&self, read: Read, error: &SourceError) -> String {
        format!("{} {error}", self.name(read))
    }

    /// The source's version, read once. Never a gate: the run goes on
    /// whatever it is. Only a recognised version text is shown; anything else
    /// is unknown with a reason and a gap, and is never echoed.
    fn read_version(&self) -> VersionRead {
        let tested = self.source.tested_version();
        let why = match self.source.version(self.deadline) {
            Ok(text) if is_version_text(&text) => {
                return VersionRead {
                    tested: text == tested,
                    version: Fact::Known(text),
                    gap: None,
                };
            }
            Ok(_) => format!(
                "{} printed text that is not a recognised version (withheld)",
                self.name(Read::Version)
            ),
            Err(e) => format!("{} {}", self.name(Read::Version), version_failure(&e)),
        };
        VersionRead {
            version: Fact::Unknown(why.clone()),
            tested: false,
            gap: Some(gap(GapKind::UntestedVersion, None, why)),
        }
    }

    /// The project list of this run: names by id.
    fn projects(
        &self,
        include_hidden: bool,
        gaps: &mut Vec<Gap>,
    ) -> Listed<BTreeMap<ProjectId, Fact<String>>> {
        let rows = self.list(Read::Projects, gaps, |deadline| {
            self.source.list_projects(include_hidden, deadline)
        })?;
        let mut names = BTreeMap::new();
        for ProjectRecord { id, name } in rows {
            if id.0.chars().count() > FIELD_MAX_CHARS {
                gaps.push(gap(
                    GapKind::Malformed,
                    Some(self.name(Read::Projects).to_owned()),
                    format!("a project id is longer than {FIELD_MAX_CHARS} characters"),
                ));
            } else {
                names.entry(id).or_insert_with(|| cap_fact(&name));
            }
        }
        Ok(names)
    }

    /// The machine list of this run: names by id.
    fn machines(&self, gaps: &mut Vec<Gap>) -> Listed<BTreeMap<String, Fact<String>>> {
        let rows = self.list(Read::Machines, gaps, |deadline| {
            self.source.list_machines(deadline)
        })?;
        let mut names = BTreeMap::new();
        for MachineRecord { id, name } in rows {
            names.entry(id).or_insert_with(|| cap_fact(&name));
        }
        Ok(names)
    }

    /// Reads a supporting list. A failure is a gap, and the reason the list
    /// is missing.
    fn list<T>(
        &self,
        read: Read,
        gaps: &mut Vec<Gap>,
        call: impl FnOnce(Instant) -> Result<Rows<T>, SourceError>,
    ) -> Listed<Vec<T>> {
        let subject = Some(self.name(read).to_owned());
        if self.out_of_budget() {
            gaps.push(gap(
                GapKind::BudgetExhausted,
                subject,
                NOT_READ_BUDGET.to_owned(),
            ));
            return Err(format!("{} {NOT_READ_BUDGET}", self.name(read)));
        }
        match call(self.deadline) {
            Ok(rows) => {
                gaps.extend(rows.unreadable);
                Ok(rows.records)
            }
            Err(e) => {
                let why = self.describe(read, &e);
                gaps.push(gap(kind_of(&e), subject, why.clone()));
                Err(why)
            }
        }
    }

    /// `thread show` (or the source's detail read) for `ids`, at most
    /// [`MAX_CONCURRENT_DETAIL_READS`] at once, none started after the
    /// deadline.
    fn details(&self, ids: &[SessionId]) -> Vec<Outcome> {
        let next = AtomicUsize::new(0);
        let results: Vec<Mutex<Option<Outcome>>> = ids.iter().map(|_| Mutex::new(None)).collect();
        thread::scope(|scope| {
            for _ in 0..MAX_CONCURRENT_DETAIL_READS.min(ids.len()) {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, atomic::Ordering::SeqCst);
                        let Some(id) = ids.get(i) else { break };
                        let outcome = if self.out_of_budget() {
                            Outcome::NotRead
                        } else {
                            match self.source.session_detail(id, self.deadline) {
                                Ok(detail) => Outcome::Read(Box::new(detail)),
                                Err(e) => Outcome::Failed(e),
                            }
                        };
                        *results[i].lock().unwrap_or_else(|p| p.into_inner()) = Some(outcome);
                    }
                });
            }
        });
        results
            .into_iter()
            .map(|r| {
                r.into_inner()
                    .unwrap_or_else(|p| p.into_inner())
                    .unwrap_or(Outcome::NotRead)
            })
            .collect()
    }

    /// The reported configuration from one detail read, and its gaps.
    fn configuration(
        &self,
        id: &SessionId,
        outcome: Outcome,
        gaps: &mut Vec<Gap>,
    ) -> ReportedConfiguration {
        match outcome {
            Outcome::Read(detail) => {
                let detail = *detail;
                gaps.extend(detail.session.problems);
                ReportedConfiguration {
                    model: cap_fact(&detail.model),
                    effort: cap_fact(&detail.effort),
                }
            }
            Outcome::Failed(e) => {
                let why = self.describe(Read::SessionDetail, &e);
                gaps.push(gap(kind_of(&e), Some(id.0.clone()), why.clone()));
                not_read(cap_field(&why))
            }
            Outcome::NotRead => {
                gaps.push(gap(
                    GapKind::BudgetExhausted,
                    Some(id.0.clone()),
                    NOT_READ_BUDGET.to_owned(),
                ));
                not_read(NOT_READ_BUDGET.to_owned())
            }
        }
    }

    /// The final output and the reported error of one session.
    fn output(
        &self,
        id: &SessionId,
        gaps: &mut Vec<Gap>,
    ) -> (Fact<FinalOutput>, Fact<Option<BoundedText>>) {
        let unknown = |why: String| (Fact::Unknown(why.clone()), Fact::Unknown(why));
        if self.out_of_budget() {
            gaps.push(gap(
                GapKind::BudgetExhausted,
                Some(id.0.clone()),
                format!("{} {NOT_READ_BUDGET}", self.name(Read::SessionOutput)),
            ));
            return unknown(NOT_READ_BUDGET.to_owned());
        }
        match self.source.session_output(id, self.deadline) {
            Ok(record) => {
                gaps.extend(record.problems);
                let output = match record.output {
                    Fact::Known(Some(text)) if !text.is_empty() => Fact::Known(
                        FinalOutput::Reported(BoundedText::bytes(&text, FINAL_OUTPUT_MAX_BYTES)),
                    ),
                    Fact::Known(_) => Fact::Known(FinalOutput::NoFinalOutputYet),
                    Fact::Unknown(why) => Fact::Unknown(cap_field(&why)),
                };
                let error = match record.error {
                    Fact::Known(error) => {
                        Fact::Known(error.map(|text| BoundedText::chars(&text, FIELD_MAX_CHARS)))
                    }
                    Fact::Unknown(why) => Fact::Unknown(cap_field(&why)),
                };
                (output, error)
            }
            Err(e) => {
                let why = self.describe(Read::SessionOutput, &e);
                gaps.push(gap(kind_of(&e), Some(id.0.clone()), why.clone()));
                unknown(cap_field(&why))
            }
        }
    }

    fn envelope<T>(
        &self,
        version: VersionRead,
        counts: Option<Counts>,
        gaps: Vec<Gap>,
        data: T,
    ) -> Envelope<T> {
        let mut gaps = gaps;
        gaps.extend(version.gap);
        let gaps = merge_gaps(gaps);
        let state = if gaps.is_empty() {
            Completeness::Complete
        } else {
            Completeness::Partial
        };
        Envelope {
            schema: SCHEMA_ID.to_owned(),
            source: SourceInfo {
                name: self.source.name().to_owned(),
                version: version.version,
                tested_version: self.source.tested_version().to_owned(),
                version_tested: version.tested,
            },
            coverage: Coverage { state, counts },
            freshness: Freshness {
                snapshot_at: self.snapshot,
            },
            gaps,
            data,
        }
    }
}

fn build_session(
    record: &SessionRecord,
    reported_configuration: ReportedConfiguration,
    projects: &Listed<BTreeMap<ProjectId, Fact<String>>>,
    machines: &Listed<BTreeMap<String, Fact<String>>>,
    snapshot: Timestamp,
) -> Session {
    let group = match (&record.project_id, projects) {
        (Fact::Known(id), Ok(names)) => match names.get(id) {
            Some(name) => Grouping::Project {
                id: id.clone(),
                name: name.clone(),
            },
            None => Grouping::Ungrouped {
                reason: cap_field(&format!(
                    "project {} is not in the project list read in this run",
                    id.0
                )),
            },
        },
        (Fact::Known(id), Err(why)) => Grouping::Ungrouped {
            reason: cap_field(&format!("project {} cannot be verified: {why}", id.0)),
        },
        (Fact::Unknown(why), _) => Grouping::Ungrouped {
            reason: cap_field(&format!("project unknown ({why})")),
        },
    };
    let location = match &record.placement {
        Fact::Known(Placement::Cloud) => Fact::Known(Location::Cloud),
        Fact::Known(Placement::Host(host)) => {
            let machine = match machines {
                Ok(names) => names.get(host).cloned().unwrap_or_else(|| {
                    Fact::Unknown(cap_field(&format!(
                        "host {host} is not in the machine list read in this run"
                    )))
                }),
                Err(why) => {
                    Fact::Unknown(cap_field(&format!("host {host} cannot be matched: {why}")))
                }
            };
            Fact::Known(Location::Local { machine })
        }
        Fact::Unknown(why) => Fact::Unknown(cap_field(why)),
    };
    let recap = match &record.purpose_text {
        Fact::Known(Some(text)) if !text.trim().is_empty() => Fact::Known(
            Recap::PurposeAsReported(BoundedText::chars(text, PURPOSE_MAX_CHARS)),
        ),
        Fact::Known(_) => Fact::Known(Recap::NoRecapYet),
        Fact::Unknown(why) => Fact::Unknown(cap_field(why)),
    };
    let parent = match &record.parent {
        Fact::Known(parent) => Fact::Known(parent.as_ref().map(|id| SessionId(cap_field(&id.0)))),
        Fact::Unknown(why) => Fact::Unknown(cap_field(why)),
    };
    let updated_at = match &record.updated_at {
        Fact::Known(at) => Fact::Known(*at),
        Fact::Unknown(why) => Fact::Unknown(cap_field(why)),
    };
    let age_ms = match &updated_at {
        Fact::Known(at) => match snapshot.0.checked_sub(at.0) {
            Some(age) => Fact::Known(age),
            None => Fact::Unknown(format!(
                "the update time {} is too far from the snapshot to give an age",
                at.0
            )),
        },
        Fact::Unknown(why) => Fact::Unknown(why.clone()),
    };
    Session {
        id: record.id.clone(),
        title: cap_fact(&record.title),
        provider: cap_fact(&record.provider),
        status: cap_fact(&record.status),
        location,
        group,
        parent,
        hidden: cap_reason(&record.hidden),
        archived: cap_reason(&record.archived),
        recap,
        reported_configuration,
        updated_at,
        age_ms,
    }
}

/// Projects among `groups` whose name another project in `projects` shares.
fn shared_names<'g>(
    projects: &Listed<BTreeMap<ProjectId, Fact<String>>>,
    groups: impl IntoIterator<Item = &'g Grouping>,
) -> BTreeSet<ProjectId> {
    let Ok(names) = projects else {
        return BTreeSet::new();
    };
    groups
        .into_iter()
        .filter_map(|group| match group {
            Grouping::Project { id, name } => {
                let sharing = names.values().filter(|other| *other == name).count();
                (sharing > 1).then(|| id.clone())
            }
            Grouping::Ungrouped { .. } => None,
        })
        .collect()
}

fn not_read(why: String) -> ReportedConfiguration {
    ReportedConfiguration {
        model: Fact::Unknown(why.clone()),
        effort: Fact::Unknown(why),
    }
}

fn gap(kind: GapKind, subject: Option<String>, detail: String) -> Gap {
    Gap {
        kind,
        subject,
        detail,
    }
}

fn kind_of(error: &SourceError) -> GapKind {
    match error {
        SourceError::NotFound { .. } => GapKind::NotFound,
        SourceError::InvalidId { .. } | SourceError::Malformed { .. } => GapKind::Malformed,
        SourceError::CommandFailed { .. } | SourceError::Unavailable { .. } => {
            GapKind::CommandFailed
        }
        SourceError::TimedOut { .. } => GapKind::TimedOut,
    }
}

/// One gap per kind and detail: the same problem in many rows is one gap,
/// whose subject lists them. Every subject and detail is capped.
fn merge_gaps(gaps: Vec<Gap>) -> Vec<Gap> {
    let mut merged: Vec<(GapKind, String, Vec<String>)> = Vec::new();
    let mut index: HashMap<(GapKind, String), (usize, HashSet<String>)> = HashMap::new();
    for Gap {
        kind,
        subject,
        detail,
    } in gaps
    {
        let (i, seen) = index.entry((kind, detail.clone())).or_insert_with(|| {
            merged.push((kind, detail, Vec::new()));
            (merged.len() - 1, HashSet::new())
        });
        if let Some(subject) = subject.filter(|s| seen.insert(s.clone())) {
            merged[*i].2.push(subject);
        }
    }
    merged
        .into_iter()
        .map(|(kind, detail, subjects)| Gap {
            kind,
            subject: match subjects.len() {
                0 => None,
                1 => Some(cap_field(&subjects[0])),
                n => Some(cap_field(&format!("{n} subjects: {}", subjects.join(", ")))),
            },
            detail: cap_field(&detail),
        })
        .collect()
}

fn cap_fact(fact: &Fact<String>) -> Fact<String> {
    match fact {
        Fact::Known(text) => Fact::Known(cap_field(text)),
        Fact::Unknown(why) => Fact::Unknown(cap_field(why)),
    }
}

fn cap_reason<T: Clone>(fact: &Fact<T>) -> Fact<T> {
    match fact {
        Fact::Known(value) => Fact::Known(value.clone()),
        Fact::Unknown(why) => Fact::Unknown(cap_field(why)),
    }
}

fn cap_error(error: SourceError) -> SourceError {
    match error {
        SourceError::NotFound { subject } => SourceError::NotFound {
            subject: cap_field(&subject),
        },
        SourceError::InvalidId { id, detail } => SourceError::InvalidId {
            id: cap_field(&id),
            detail: cap_field(&detail),
        },
        SourceError::CommandFailed { exit_code, detail } => SourceError::CommandFailed {
            exit_code,
            detail: cap_field(&detail),
        },
        SourceError::TimedOut { after_ms } => SourceError::TimedOut { after_ms },
        SourceError::Malformed { detail } => SourceError::Malformed {
            detail: cap_field(&detail),
        },
        SourceError::Unavailable { detail } => SourceError::Unavailable {
            detail: cap_field(&detail),
        },
    }
}
