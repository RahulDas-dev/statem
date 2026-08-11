//! AG-UI protocol event structures used by [`crate::machine::StateMachine::stream`].
//!
//! Hand-rolled rather than pulled from a dependency: the official `ag-ui-protocol` crate (and
//! every other AG-UI crate checked on crates.io as of this writing) requires Cargo edition2024,
//! which isn't stabilized under this crate's pinned toolchain (rustc 1.82). Field names and the
//! flattened `BaseEvent`/camelCase shape below are confirmed against that official crate's
//! source and the Python `ag_ui.core` package, so the wire format still matches the real
//! protocol -- only the crate boundary differs.
//!
//! This whole module (and the `stream` method that uses it) is gated behind the `agui` Cargo
//! feature, mirroring the Python port's lazy `from . import agui` inside `stream()`: plain use
//! of `statem-rs` never pulls in `json-patch`/`async-stream`/`futures-*`.

use serde::Serialize;
use serde_json::Value;

use crate::schema::{ResultEntry, ResultKind};

/// Fields every AG-UI event carries, flattened into each variant's own struct.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BaseEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartedEvent {
    pub step_name: String,
    #[serde(flatten)]
    pub base: BaseEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFinishedEvent {
    pub step_name: String,
    #[serde(flatten)]
    pub base: BaseEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSnapshotEvent {
    pub snapshot: Value,
    #[serde(flatten)]
    pub base: BaseEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDeltaEvent {
    /// RFC 6902 JSON Patch array (see [`diff_state`]).
    pub delta: Value,
    #[serde(flatten)]
    pub base: BaseEvent,
}

/// `content` payload for an [`ActivitySnapshotEvent`]: a single guard/action result.
///
/// `result` is `Some(bool)` for a guard, `None` for an action -- this port doesn't record an
/// action's return value (see [`ResultKind`]'s docs), unlike the Python original where `result`
/// can be any JSON-serializable action return value.
#[derive(Debug, Clone, Serialize)]
pub struct ActivityContent {
    #[serde(rename = "type")]
    pub content_type: &'static str,
    pub kind: &'static str,
    pub name: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshotEvent {
    pub message_id: String,
    pub activity_type: &'static str,
    pub content: ActivityContent,
    pub replace: bool,
    #[serde(flatten)]
    pub base: BaseEvent,
}

/// One AG-UI event, as yielded by [`crate::machine::StateMachine::stream`]. Serializes with an
/// internal `"type"` tag matching the AG-UI wire format (e.g. `{"type":"STEP_STARTED",...}`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AguiEvent {
    #[serde(rename = "STEP_STARTED")]
    StepStarted(StepStartedEvent),
    #[serde(rename = "STEP_FINISHED")]
    StepFinished(StepFinishedEvent),
    #[serde(rename = "STATE_SNAPSHOT")]
    StateSnapshot(StateSnapshotEvent),
    #[serde(rename = "STATE_DELTA")]
    StateDelta(StateDeltaEvent),
    #[serde(rename = "ACTIVITY_SNAPSHOT")]
    ActivitySnapshot(ActivitySnapshotEvent),
}

pub(crate) fn step_started(step_name: impl Into<String>) -> AguiEvent {
    AguiEvent::StepStarted(StepStartedEvent { step_name: step_name.into(), base: BaseEvent::default() })
}

pub(crate) fn step_finished(step_name: impl Into<String>) -> AguiEvent {
    AguiEvent::StepFinished(StepFinishedEvent { step_name: step_name.into(), base: BaseEvent::default() })
}

pub(crate) fn state_snapshot(snapshot: Value) -> AguiEvent {
    AguiEvent::StateSnapshot(StateSnapshotEvent { snapshot, base: BaseEvent::default() })
}

pub(crate) fn state_delta(delta: Value) -> AguiEvent {
    AguiEvent::StateDelta(StateDeltaEvent { delta, base: BaseEvent::default() })
}

/// RFC 6902 patch turning `old` into `new`, via the `json-patch` crate -- the Rust equivalent of
/// Python's `jsonpatch.make_patch(old, new).patch`.
pub(crate) fn diff_state(old: &Value, new: &Value) -> Value {
    serde_json::to_value(json_patch::diff(old, new)).expect("Patch always serializes to a JSON array")
}

pub(crate) fn activity(entry: &ResultEntry) -> AguiEvent {
    let (kind, result) = match entry.kind {
        ResultKind::Guard(passed) => ("guard", Some(passed)),
        ResultKind::Action => ("action", None),
    };
    AguiEvent::ActivitySnapshot(ActivitySnapshotEvent {
        message_id: crate::schema::generate_run_id(),
        activity_type: kind,
        content: ActivityContent {
            content_type: "activity",
            kind,
            name: entry.name.clone(),
            source: entry.source.to_string(),
            result,
        },
        replace: false,
        base: BaseEvent::default(),
    })
}
