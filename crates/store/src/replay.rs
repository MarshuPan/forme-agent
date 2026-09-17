use forme_protocol as p;

use crate::{SchemaSnapshot, SessionState, Transcript, TranscriptEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectionName {
    SessionState,
    Transcript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SessionStatePath {
    Root,
    SchemaVersion,
    RunId,
    Source,
    Status,
    SessionRef,
    Workspace,
    WaitReason,
    ResumeRef,
    StopReason,
    ResultRef,
    LastStreamSeq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TranscriptPath {
    Root,
    SchemaVersion,
    RunId,
    Entries,
    LastStreamSeq,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectionPath {
    SessionState(SessionStatePath),
    Transcript(TranscriptPath),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionValue {
    Absent,
    SchemaVersion(p::SchemaVersion),
    RunId(p::RunId),
    Source(p::Source),
    RunStatus(p::RunStatus),
    SessionId(p::SessionId),
    Workspace(p::WorkspaceRef),
    WaitReason(p::WaitReason),
    ResumeRef(p::ResumeRef),
    StopReason(p::StopReason),
    EventId(p::EventId),
    StreamSeq(u64),
    TranscriptEntries(Vec<TranscriptEntry>),
    SessionState(SessionState),
    Transcript(Transcript),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionDiff {
    pub projection: ProjectionName,
    pub path: ProjectionPath,
    pub current: ProjectionValue,
    pub replayed: ProjectionValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayReport {
    pub run: p::RunId,
    pub schema_snapshot: SchemaSnapshot,
    pub diff_vs_current: Vec<ProjectionDiff>,
    pub replayed_session_state: SessionState,
    pub replayed_transcript: Transcript,
}

pub(crate) fn projection_diff(
    current_session: Option<&SessionState>,
    replayed_session: &SessionState,
    current_transcript: Option<&Transcript>,
    replayed_transcript: &Transcript,
) -> Vec<ProjectionDiff> {
    let mut diffs = Vec::new();
    diff_session_state(&mut diffs, current_session, replayed_session);
    diff_transcript(&mut diffs, current_transcript, replayed_transcript);
    diffs.sort_by(|left, right| {
        left.projection
            .cmp(&right.projection)
            .then_with(|| left.path.cmp(&right.path))
    });
    diffs
}

fn diff_session_state(
    diffs: &mut Vec<ProjectionDiff>,
    current: Option<&SessionState>,
    replayed: &SessionState,
) {
    let Some(current) = current else {
        push_diff(
            diffs,
            ProjectionName::SessionState,
            ProjectionPath::SessionState(SessionStatePath::Root),
            ProjectionValue::Absent,
            ProjectionValue::SessionState(replayed.clone()),
        );
        return;
    };

    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::SchemaVersion),
        ProjectionValue::SchemaVersion(current.schema_version),
        ProjectionValue::SchemaVersion(replayed.schema_version),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::RunId),
        option_value(current.run_id.clone(), ProjectionValue::RunId),
        option_value(replayed.run_id.clone(), ProjectionValue::RunId),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::Source),
        option_value(current.source, ProjectionValue::Source),
        option_value(replayed.source, ProjectionValue::Source),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::Status),
        ProjectionValue::RunStatus(current.status),
        ProjectionValue::RunStatus(replayed.status),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::SessionRef),
        option_value(current.session_ref.clone(), ProjectionValue::SessionId),
        option_value(replayed.session_ref.clone(), ProjectionValue::SessionId),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::Workspace),
        option_value(current.workspace.clone(), ProjectionValue::Workspace),
        option_value(replayed.workspace.clone(), ProjectionValue::Workspace),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::WaitReason),
        option_value(current.wait_reason.clone(), ProjectionValue::WaitReason),
        option_value(replayed.wait_reason.clone(), ProjectionValue::WaitReason),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::ResumeRef),
        option_value(current.resume_ref.clone(), ProjectionValue::ResumeRef),
        option_value(replayed.resume_ref.clone(), ProjectionValue::ResumeRef),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::StopReason),
        option_value(current.stop_reason.clone(), ProjectionValue::StopReason),
        option_value(replayed.stop_reason.clone(), ProjectionValue::StopReason),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::ResultRef),
        option_value(current.result_ref.clone(), ProjectionValue::EventId),
        option_value(replayed.result_ref.clone(), ProjectionValue::EventId),
    );
    push_changed(
        diffs,
        ProjectionName::SessionState,
        ProjectionPath::SessionState(SessionStatePath::LastStreamSeq),
        ProjectionValue::StreamSeq(current.last_stream_seq),
        ProjectionValue::StreamSeq(replayed.last_stream_seq),
    );
}

fn diff_transcript(
    diffs: &mut Vec<ProjectionDiff>,
    current: Option<&Transcript>,
    replayed: &Transcript,
) {
    let Some(current) = current else {
        push_diff(
            diffs,
            ProjectionName::Transcript,
            ProjectionPath::Transcript(TranscriptPath::Root),
            ProjectionValue::Absent,
            ProjectionValue::Transcript(replayed.clone()),
        );
        return;
    };

    push_changed(
        diffs,
        ProjectionName::Transcript,
        ProjectionPath::Transcript(TranscriptPath::SchemaVersion),
        ProjectionValue::SchemaVersion(current.schema_version),
        ProjectionValue::SchemaVersion(replayed.schema_version),
    );
    push_changed(
        diffs,
        ProjectionName::Transcript,
        ProjectionPath::Transcript(TranscriptPath::RunId),
        option_value(current.run_id.clone(), ProjectionValue::RunId),
        option_value(replayed.run_id.clone(), ProjectionValue::RunId),
    );
    push_changed(
        diffs,
        ProjectionName::Transcript,
        ProjectionPath::Transcript(TranscriptPath::Entries),
        ProjectionValue::TranscriptEntries(current.entries.clone()),
        ProjectionValue::TranscriptEntries(replayed.entries.clone()),
    );
    push_changed(
        diffs,
        ProjectionName::Transcript,
        ProjectionPath::Transcript(TranscriptPath::LastStreamSeq),
        ProjectionValue::StreamSeq(current.last_stream_seq),
        ProjectionValue::StreamSeq(replayed.last_stream_seq),
    );
}

fn option_value<T>(value: Option<T>, wrap: fn(T) -> ProjectionValue) -> ProjectionValue {
    value.map(wrap).unwrap_or(ProjectionValue::Absent)
}

fn push_changed(
    diffs: &mut Vec<ProjectionDiff>,
    projection: ProjectionName,
    path: ProjectionPath,
    current: ProjectionValue,
    replayed: ProjectionValue,
) {
    if current != replayed {
        push_diff(diffs, projection, path, current, replayed);
    }
}

fn push_diff(
    diffs: &mut Vec<ProjectionDiff>,
    projection: ProjectionName,
    path: ProjectionPath,
    current: ProjectionValue,
    replayed: ProjectionValue,
) {
    diffs.push(ProjectionDiff {
        projection,
        path,
        current,
        replayed,
    });
}
