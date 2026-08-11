//! Core types: signals, execution context, config models, and errors.
//!
//! Mirrors `statem/schema.py` in the Python package. Config shorthand normalization (a
//! transition candidate written as a bare string, a single dict, or a list of either) is
//! replicated via `serde`'s `#[serde(untagged)]` + `#[serde(from = "...")]` idioms instead of
//! Python's `field_validator(mode="before")`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;
use serde::Deserialize;

/// Generate a `run_id`: a nanosecond timestamp combined with a monotonic in-process counter,
/// formatted as hex. Not a crypto-strength unique id (unlike Python's `uuid4`) -- just good
/// enough for log correlation, and dependency-free (see the crate root's dependency note on why
/// `uuid` was dropped).
pub(crate) fn generate_run_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:x}-{count:x}")
}

/// A signal that triggers a transition.
#[derive(Debug, Clone)]
pub struct Signal {
    /// Event name (upper-case by convention, e.g. `"START"`).
    pub event: String,
    /// Arbitrary payload; defaults to empty.
    pub data: HashMap<String, serde_json::Value>,
}

impl Signal {
    pub fn new(event: impl Into<String>) -> Self {
        Signal { event: event.into(), data: HashMap::new() }
    }

    pub fn with_data(event: impl Into<String>, data: HashMap<String, serde_json::Value>) -> Self {
        Signal { event: event.into(), data }
    }
}

/// Which lifecycle hook produced a [`ResultEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookSource {
    On,
    Always,
    Entry,
    Exit,
}

/// What kind of check/effect a [`ResultEntry`] records.
///
/// Unlike the Python port, a successful action's return value isn't recorded here (see the
/// design notes in the crate root) -- only whether a guard passed/failed, or that an action ran.
/// A *failed* action never gets a `ResultEntry` at all: it aborts the transition attempt
/// immediately, mirroring the Python engine (a failed action's `ResultEntry.append` call never
/// runs since the exception propagates first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultKind {
    Guard(bool),
    Action,
}

/// A single recorded guard check or action run, in execution order.
#[derive(Debug, Clone)]
pub struct ResultEntry {
    pub state: String,
    pub source: HookSource,
    pub name: String,
    pub kind: ResultKind,
}

/// Derives the value broadcast via `STATE_SNAPSHOT`/`STATE_DELTA` from `session`, passed to
/// [`crate::machine::StateMachine::stream`].
#[cfg(feature = "agui")]
pub type StateAccessor<T> = std::sync::Arc<dyn Fn(&T) -> serde_json::Value + Send + Sync>;

/// Short-lived execution context passed to every guard and action during one `run()` call.
///
/// Generic over `T`, the session type. Guards receive `&Context<T>` (read-only); actions receive
/// `&mut Context<T>` (may mutate `session` freely) -- enforced by the borrow checker, not just by
/// convention as in the Python port.
#[derive(Clone)]
pub struct Context<T> {
    /// Correlation id for this run, used in log lines. `None` until `StateMachine::run` fills it
    /// in (generates one if the caller didn't supply one).
    pub run_id: Option<String>,
    /// Correlation id for the broader conversation/session this run belongs to (one thread can
    /// span many runs). `None` until `StateMachine::run`/`stream` fills it in, same as `run_id`.
    pub thread_id: Option<String>,
    pub current_state: String,
    pub session: T,
    /// Every state entered during this run, in order; the first entry is always the initial state.
    pub history: Vec<String>,
    pub results: Vec<ResultEntry>,
    /// Internal -- set by `StateMachine::stream` to a sender it emits AG-UI events onto; left
    /// `None` by `StateMachine::run`. Not meant for guards/actions to use directly; check
    /// [`Context::is_stream`] instead of reading this field.
    #[cfg(feature = "agui")]
    pub(crate) event_tx: Option<futures_channel::mpsc::UnboundedSender<crate::agui::AguiEvent>>,
    /// Internal -- set by `StateMachine::stream` from its `state_accessor` argument. Derives the
    /// value broadcast via `STATE_SNAPSHOT`/`STATE_DELTA` from `session`; `None` means the
    /// default `{"current_state": ...}` view is used instead.
    #[cfg(feature = "agui")]
    pub(crate) state_accessor: Option<StateAccessor<T>>,
    /// Internal -- the last value broadcast via `STATE_SNAPSHOT`/`STATE_DELTA`, kept so
    /// `StateMachine::stream` can diff against it to build the next `STATE_DELTA`.
    #[cfg(feature = "agui")]
    pub(crate) state_snapshot: Option<serde_json::Value>,
}

impl<T> Context<T> {
    pub fn new(current_state: impl Into<String>, session: T, run_id: Option<String>, thread_id: Option<String>) -> Self {
        let current_state = current_state.into();
        Context {
            run_id,
            thread_id,
            history: vec![current_state.clone()],
            current_state,
            session,
            results: Vec::new(),
            #[cfg(feature = "agui")]
            event_tx: None,
            #[cfg(feature = "agui")]
            state_accessor: None,
            #[cfg(feature = "agui")]
            state_snapshot: None,
        }
    }

    /// `true` when this context was created by `StateMachine::stream`, `false` for `run`.
    #[cfg(feature = "agui")]
    pub(crate) fn is_stream(&self) -> bool {
        self.event_tx.is_some()
    }
}

/// Hand-written rather than derived: under the `agui` feature, `state_accessor` is a
/// `dyn Fn(&T) -> Value`, which can't implement `Debug` -- printed as presence/absence instead.
impl<T: std::fmt::Debug> std::fmt::Debug for Context<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Context");
        s.field("run_id", &self.run_id)
            .field("thread_id", &self.thread_id)
            .field("current_state", &self.current_state)
            .field("session", &self.session)
            .field("history", &self.history)
            .field("results", &self.results);
        #[cfg(feature = "agui")]
        s.field("is_stream", &self.is_stream()).field("state_snapshot", &self.state_snapshot);
        s.finish()
    }
}

/// Configuration for a single transition candidate.
#[derive(Debug, Clone, Deserialize)]
pub struct TransitionConfig {
    /// Name of the state to transition to.
    pub target: String,
    /// Registered guard name that must pass for this candidate to fire; `None` always passes.
    #[serde(default)]
    pub guard: Option<String>,
    /// Ordered action names to run when this candidate fires.
    #[serde(default)]
    pub actions: Vec<String>,
}

/// Deserialization-only shorthand for one transition candidate: a bare target string, or the
/// full `{target, guard, actions}` form.
#[derive(Deserialize)]
#[serde(untagged)]
enum TransitionShorthand {
    Target(String),
    Full(TransitionConfig),
}

impl From<TransitionShorthand> for TransitionConfig {
    fn from(value: TransitionShorthand) -> Self {
        match value {
            TransitionShorthand::Target(target) => TransitionConfig { target, guard: None, actions: Vec::new() },
            TransitionShorthand::Full(config) => config,
        }
    }
}

/// Deserialization-only shorthand for one event's `on` value: a single candidate, or a list of
/// candidates (a guard chain).
#[derive(Deserialize)]
#[serde(untagged)]
enum OnValue {
    Single(TransitionShorthand),
    Many(Vec<TransitionShorthand>),
}

impl From<OnValue> for Vec<TransitionConfig> {
    fn from(value: OnValue) -> Self {
        match value {
            OnValue::Single(candidate) => vec![candidate.into()],
            OnValue::Many(candidates) => candidates.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Deserialize)]
struct StateConfigRaw {
    #[serde(default)]
    on: IndexMap<String, OnValue>,
    #[serde(default)]
    always: Vec<TransitionShorthand>,
    #[serde(default)]
    entry: Vec<String>,
    #[serde(default)]
    exit: Vec<String>,
    #[serde(default)]
    error_state: Option<String>,
}

impl From<StateConfigRaw> for StateConfig {
    fn from(raw: StateConfigRaw) -> Self {
        StateConfig {
            on: raw.on.into_iter().map(|(event, value)| (event, value.into())).collect(),
            always: raw.always.into_iter().map(Into::into).collect(),
            entry: raw.entry,
            exit: raw.exit,
            error_state: raw.error_state,
        }
    }
}

/// Configuration for one state.
///
/// Each `on` value and each `always` item is normalized from shorthand at deserialization time
/// (see [`TransitionShorthand`]/[`OnValue`]) -- by the time you hold a `StateConfig`, `on` is
/// always `event -> Vec<TransitionConfig>` and `always` is always `Vec<TransitionConfig>`.
///
/// `on` is an [`IndexMap`], not a `HashMap`: it's iterated for output (error messages, the
/// `diagram` module's edge ordering), where declaration order matters for readability and for
/// getting the same output on every run -- `HashMap`'s iteration order isn't guaranteed. `Signal`'s
/// `data` field, by contrast, is a plain `HashMap`: it's only ever looked up by key, never
/// iterated for display, so there's no reason to pay for order-preservation there.
#[derive(Debug, Clone, Deserialize)]
#[serde(from = "StateConfigRaw")]
pub struct StateConfig {
    pub on: IndexMap<String, Vec<TransitionConfig>>,
    pub always: Vec<TransitionConfig>,
    pub entry: Vec<String>,
    pub exit: Vec<String>,
    pub error_state: Option<String>,
}

impl StateConfig {
    /// Event names this state can receive via `on`, excluding the `"*"` wildcard.
    pub fn available_events(&self) -> Vec<String> {
        self.on.keys().filter(|name| name.as_str() != "*").cloned().collect()
    }

    /// Whether this state has a `"*"` catch-all handler.
    pub fn accepts_wildcard(&self) -> bool {
        self.on.contains_key("*")
    }
}

/// Raised by [`crate::machine::StateMachine::from_config`] when a transition target (in `on`,
/// `always`, or `error_state`) doesn't name a real state.
#[derive(Debug, thiserror::Error)]
#[error("invalid transition targets: {0:?}")]
pub struct BuildError(pub Vec<String>);

/// Raised by [`crate::machine::StateMachine::run`].
///
/// [`RunError::ActionFailed`] from an `on`-transition attempt -- either the firing candidate's
/// own `actions`, or the `entry`/`exit` actions of the state it then enters -- is caught by the
/// state active when the signal was dispatched, if it has `error_state` set (see
/// `StateMachine::run`'s docs). `always`-transition action failures are a deliberate exception:
/// they're never caught, since `always` represents a structural, continuously-re-checked
/// invariant rather than a one-shot response to an external event -- a failure there is a
/// config/logic bug worth surfacing loudly, not something to silently route around.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("action '{action}' failed in state '{state}'")]
    ActionFailed {
        state: String,
        action: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("guard '{0}' is not registered")]
    UnknownGuard(String),
    #[error("action '{0}' is not registered")]
    UnknownAction(String),
    #[error("always-transition loop exceeded 100 hops at '{0}'")]
    AlwaysLoopExceeded(String),
}

/// Raised by [`crate::machine::StateMachine::validate_registries`]: every guard/action name
/// referenced anywhere in `config` (`on`, `always`, `entry`, `exit`) that isn't registered,
/// tagged with where it's referenced (e.g. `"idle.on.START: can_start"`).
///
/// Both lists are always present (possibly empty) rather than conditionally omitted the way the
/// Python original's joined message does -- simpler to construct, same information either way.
#[derive(Debug, thiserror::Error)]
#[error("unregistered guards: {missing_guards:?}, unregistered actions: {missing_actions:?}")]
pub struct ValidationError {
    pub missing_guards: Vec<String>,
    pub missing_actions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_run_id_produces_distinct_nonempty_ids() {
        let a = generate_run_id();
        let b = generate_run_id();
        assert!(!a.is_empty());
        assert!(!b.is_empty());
        assert_ne!(a, b);
    }

    #[test]
    fn context_preserves_explicit_run_id() {
        let ctx = Context::new("idle", (), Some("custom-id".to_string()), None);
        assert_eq!(ctx.run_id.as_deref(), Some("custom-id"));
    }

    #[test]
    fn context_leaves_run_id_none_when_omitted() {
        let ctx = Context::new("idle", (), None, None);
        assert_eq!(ctx.run_id, None);
    }

    #[test]
    fn context_preserves_explicit_thread_id() {
        let ctx = Context::new("idle", (), None, Some("custom-thread".to_string()));
        assert_eq!(ctx.thread_id.as_deref(), Some("custom-thread"));
    }
}
