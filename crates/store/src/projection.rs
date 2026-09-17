use forme_protocol as p;
use serde::{Deserialize, Serialize};

use crate::Projection;

const PROJECTION_SCHEMA_VERSION: p::SchemaVersion = p::SchemaVersion(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
    pub schema_version: p::SchemaVersion,
    pub run_id: Option<p::RunId>,
    pub source: Option<p::Source>,
    pub status: p::RunStatus,
    pub session_ref: Option<p::SessionId>,
    pub workspace: Option<p::WorkspaceRef>,
    pub wait_reason: Option<p::WaitReason>,
    pub resume_ref: Option<p::ResumeRef>,
    pub stop_reason: Option<p::StopReason>,
    pub result_ref: Option<p::EventId>,
    pub last_stream_seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStateProjection;

impl Projection for SessionStateProjection {
    type State = SessionState;

    fn empty() -> Self::State {
        SessionState {
            schema_version: PROJECTION_SCHEMA_VERSION,
            run_id: None,
            source: None,
            status: p::RunStatus::Accepted,
            session_ref: None,
            workspace: None,
            wait_reason: None,
            resume_ref: None,
            stop_reason: None,
            result_ref: None,
            last_stream_seq: 0,
        }
    }

    fn apply(state: &mut Self::State, event: &p::Event) {
        state.run_id = Some(event.run_id.clone());
        state.last_stream_seq = event.stream_seq;

        match &event.payload {
            p::EventPayload::RunAccepted(payload) => {
                state.source = Some(payload.source);
                state.session_ref = Some(payload.session_ref.clone());
                state.status = p::RunStatus::Accepted;
            }
            p::EventPayload::SessionBound(payload) => {
                state.workspace = Some(payload.workspace.clone());
                state.status = p::RunStatus::Running;
            }
            p::EventPayload::RunComplete(payload) => {
                state.status = p::RunStatus::Complete;
                state.stop_reason = Some(payload.stop_reason.clone());
                state.result_ref = payload.result_ref.clone();
            }
            p::EventPayload::RunAborted(payload) => {
                state.status = p::RunStatus::Aborted;
                state.stop_reason = Some(payload.stop_reason.clone());
                state.result_ref = payload.result_ref.clone();
            }
            p::EventPayload::RunFailed(payload) => {
                state.status = p::RunStatus::Failed;
                state.stop_reason = Some(payload.stop_reason.clone());
                state.result_ref = payload.result_ref.clone();
            }
            p::EventPayload::RunLimited(payload) => {
                state.status = p::RunStatus::Limited;
                state.stop_reason = Some(payload.stop_reason.clone());
                state.result_ref = payload.result_ref.clone();
            }
            p::EventPayload::RunWaiting(payload) => {
                state.status = p::RunStatus::Waiting;
                state.wait_reason = Some(payload.wait_reason.clone());
                state.resume_ref = Some(payload.resume_ref.clone());
            }
            p::EventPayload::RunResumed(payload) => {
                state.status = p::RunStatus::Running;
                state.wait_reason = Some(payload.wait_reason.clone());
                state.resume_ref = Some(payload.resume_ref.clone());
            }
            p::EventPayload::TurnStarted(_)
            | p::EventPayload::TurnComplete(_)
            | p::EventPayload::ContextBuildStarted(_)
            | p::EventPayload::ContextBuildFinished(_)
            | p::EventPayload::CompactionStarted(_)
            | p::EventPayload::CompactionFinished(_) => {
                state.status = p::RunStatus::Running;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    pub schema_version: p::SchemaVersion,
    pub run_id: Option<p::RunId>,
    pub entries: Vec<TranscriptEntry>,
    pub last_stream_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub event_id: p::EventId,
    pub run_id: p::RunId,
    pub turn_id: Option<p::TurnId>,
    pub stream_seq: u64,
    pub kind: p::EventKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptProjection;

impl Projection for TranscriptProjection {
    type State = Transcript;

    fn empty() -> Self::State {
        Transcript {
            schema_version: PROJECTION_SCHEMA_VERSION,
            run_id: None,
            entries: Vec::new(),
            last_stream_seq: 0,
        }
    }

    fn apply(state: &mut Self::State, event: &p::Event) {
        state.run_id = Some(event.run_id.clone());
        state.last_stream_seq = event.stream_seq;

        let text = match &event.payload {
            p::EventPayload::RunAccepted(payload) => Some(payload.input_ref.0.clone()),
            p::EventPayload::ModelCallDelta(payload) => Some(payload.delta.clone()),
            p::EventPayload::ActionOutputDelta(payload) => Some(payload.delta.clone()),
            _ => None,
        };
        if let Some(text) = text {
            state.entries.push(TranscriptEntry {
                event_id: event.event_id.clone(),
                run_id: event.run_id.clone(),
                turn_id: event.turn_id.clone(),
                stream_seq: event.stream_seq,
                kind: event.kind,
                text,
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub event_id: p::EventId,
    pub run_id: p::RunId,
    pub stream_seq: u64,
    pub kind: p::EventKind,
}

pub(crate) fn searchable_text(event: &p::Event) -> Option<&str> {
    match &event.payload {
        p::EventPayload::RunAccepted(payload) => Some(&payload.input_ref.0),
        p::EventPayload::ModelCallDelta(payload) => Some(&payload.delta),
        p::EventPayload::ActionOutputDelta(payload) => Some(&payload.delta),
        _ => None,
    }
}
