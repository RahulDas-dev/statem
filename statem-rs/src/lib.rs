//! `statem-rs` -- a minimal async state machine engine for Rust.
//!
//! A sibling of the Python [`statem`](https://pypi.org/project/statem/) package, living in this
//! same repository: the state graph is data, guard/action *behavior* is ordinary Rust code
//! registered by name. Design and module layout intentionally mirror the Python package (see
//! `statem/` at the repo root) -- `schema` for the core types and config models, `registry` for
//! the guard/action traits and registries, `machine` for the engine.
//!
//! Implemented so far: `on`-transitions (with guard chains), `entry`/`exit` actions, the
//! `always`-transition auto-advance cascade, `error_state` recovery, `validate_registries`,
//! `Context::trace()` for a human-readable run report, and `StateMachine::diagram()` for a
//! Mermaid `stateDiagram-v2` export.

#[cfg(feature = "agui")]
mod agui;
mod diagram;
mod machine;
mod registry;
mod schema;
mod tracing;

#[cfg(feature = "agui")]
pub use agui::{ActivityContent, ActivitySnapshotEvent, AguiEvent, StateDeltaEvent, StateSnapshotEvent, StepFinishedEvent, StepStartedEvent};
pub use diagram::Diagram;
pub use machine::StateMachine;
pub use registry::{Action, ActionRegistry, Guard, GuardRegistry};
pub use schema::{
    BuildError, Context, HookSource, ResultEntry, ResultKind, RunError, Signal, StateConfig, TransitionConfig, ValidationError,
};
#[cfg(feature = "agui")]
pub use schema::StateAccessor;
pub use tracing::Trace;
