//! The state machine engine.
//!
//! Mirrors `StateMachine` in `statem/machine.py`: config-driven `on`-transitions (with guard
//! chains), `entry`/`exit` actions, the `always`-transition auto-advance cascade, `error_state`
//! recovery, `validate_registries`, and Mermaid diagram export via [`StateMachine::diagram`].

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::diagram::Diagram;
use crate::registry::{Action, ActionRegistry, Guard, GuardRegistry};
use crate::schema::{generate_run_id, BuildError, Context, HookSource, RunError, Signal, StateConfig, TransitionConfig, ValidationError};

/// Cap on consecutive `always`-transition hops within one [`StateMachine::run`] call, mirroring
/// `statem/machine.py`'s `_ALWAYS_MAX_DEPTH`. An `always` chain that never settles within this
/// many hops is a configuration bug (e.g. two states whose guards both always pass into each
/// other), not something to spin on forever.
const ALWAYS_MAX_DEPTH: usize = 100;

/// A validated state graph paired with registries of named guards/actions.
///
/// Build one with [`StateMachine::from_config`], register guards/actions, then drive it with
/// [`StateMachine::run`]. Holds no per-run state (all of that lives in the [`Context`] created
/// fresh for each `run()` call) -- share a built machine across concurrent runs the idiomatic
/// Rust way, by wrapping it in `Arc<StateMachine<T>>` at the call site.
pub struct StateMachine<T> {
    config: IndexMap<String, StateConfig>,
    guards: GuardRegistry<T>,
    actions: ActionRegistry<T>,
    transition_table: HashMap<(String, String), Vec<TransitionConfig>>,
}

impl<T> StateMachine<T> {
    /// Validate every transition target (in `on`, `always`, `error_state`) names a real state,
    /// then build the flattened `(state, event) -> candidates` lookup table.
    ///
    /// `config` is an [`IndexMap`] (not `HashMap`) so error messages and [`StateMachine::diagram`]
    /// come out in declaration order and stay the same across runs -- see [`StateConfig::on`]'s
    /// docs for the full reasoning.
    pub fn from_config(config: IndexMap<String, StateConfig>) -> Result<Self, BuildError> {
        let mut bad_targets = Vec::new();

        for (state_id, state_cfg) in &config {
            for (event, candidates) in &state_cfg.on {
                for candidate in candidates {
                    if !config.contains_key(&candidate.target) {
                        bad_targets.push(format!("{state_id}.on.{event} -> '{}'", candidate.target));
                    }
                }
            }
            for (idx, candidate) in state_cfg.always.iter().enumerate() {
                if !config.contains_key(&candidate.target) {
                    bad_targets.push(format!("{state_id}.always[{idx}] -> '{}'", candidate.target));
                }
            }
            if let Some(error_state) = &state_cfg.error_state {
                if !config.contains_key(error_state) {
                    bad_targets.push(format!("{state_id}.error_state -> '{error_state}'"));
                }
            }
        }

        if !bad_targets.is_empty() {
            return Err(BuildError(bad_targets));
        }

        let mut transition_table = HashMap::new();
        for (state_id, state_cfg) in &config {
            for (event, candidates) in &state_cfg.on {
                transition_table.insert((state_id.clone(), event.clone()), candidates.clone());
            }
        }

        Ok(StateMachine { config, guards: GuardRegistry::new(), actions: ActionRegistry::new(), transition_table })
    }

    pub fn register_guard(&mut self, name: impl Into<String>, guard: impl Guard<T> + 'static) {
        self.guards.register(name, guard);
    }

    pub fn register_action(&mut self, name: impl Into<String>, action: impl Action<T> + 'static) {
        self.actions.register(name, action);
    }

    /// Event names `state_name` can receive via `on`, or `[]` if the state is unknown. Excludes
    /// the `"*"` wildcard entry.
    pub fn available_events(&self, state_name: &str) -> Vec<String> {
        self.config.get(state_name).map(|cfg| cfg.available_events()).unwrap_or_default()
    }

    /// Check that every guard/action name referenced anywhere in `config` (`on`, `always`,
    /// `entry`, `exit`) is registered. `from_config` doesn't call this automatically -- unlike
    /// targets (checked eagerly at construction), guards/actions are typically registered
    /// *after* construction via [`StateMachine::register_guard`]/[`StateMachine::register_action`],
    /// so call this once you're done registering to catch typos.
    pub fn validate_registries(&self) -> Result<(), ValidationError> {
        let mut missing_guards = Vec::new();
        let mut missing_actions = Vec::new();

        for (state_id, state_cfg) in &self.config {
            missing_actions.extend(
                state_cfg.entry.iter().filter(|name| !self.actions.has(name)).map(|name| format!("{state_id}.entry: {name}")),
            );
            missing_actions.extend(
                state_cfg.exit.iter().filter(|name| !self.actions.has(name)).map(|name| format!("{state_id}.exit: {name}")),
            );

            for (event, candidates) in &state_cfg.on {
                for candidate in candidates {
                    if let Some(guard) = &candidate.guard {
                        if !self.guards.has(guard) {
                            missing_guards.push(format!("{state_id}.on.{event}: {guard}"));
                        }
                    }
                    missing_actions.extend(
                        candidate.actions.iter().filter(|name| !self.actions.has(name)).map(|name| format!("{state_id}.on.{event}: {name}")),
                    );
                }
            }

            for (idx, candidate) in state_cfg.always.iter().enumerate() {
                if let Some(guard) = &candidate.guard {
                    if !self.guards.has(guard) {
                        missing_guards.push(format!("{state_id}.always[{idx}]: {guard}"));
                    }
                }
                missing_actions.extend(
                    candidate.actions.iter().filter(|name| !self.actions.has(name)).map(|name| format!("{state_id}.always[{idx}]: {name}")),
                );
            }
        }

        if missing_guards.is_empty() && missing_actions.is_empty() {
            Ok(())
        } else {
            Err(ValidationError { missing_guards, missing_actions })
        }
    }

    /// Render this machine's config as a Mermaid `stateDiagram-v2` diagram (see [`Diagram`]).
    /// `initial`, if given and present in `config`, adds a `[*] --> initial` entry-point edge.
    pub fn diagram(&self, initial: Option<&str>) -> Diagram<'_> {
        Diagram::new(&self.config, initial)
    }
}

impl<T: Send + Sync> StateMachine<T> {
    /// Process one or many signals starting from `state_name`.
    ///
    /// `run_id` is an optional correlation id; a fresh one is generated when omitted. `events`
    /// may be empty to only resolve pending `always`-transitions for the current state.
    ///
    /// Returns the whole [`Context<T>`] once every signal has been processed and all pending
    /// `always`-transitions have settled -- not just the final state name. `session` is moved in
    /// by value and owned by the returned `Context`, so this is how you get it (with whatever
    /// your actions mutated on it) back, along with `history`/`results`/[`Context::trace`]. This
    /// is a deliberate departure from the Python port, where `run()` returns only the final state
    /// name: there, `session` is an object passed by reference, so the caller's copy is already
    /// mutated in place regardless of what `run()` returns. Rust's ownership model has no
    /// equivalent -- `session` moved into `run()` and never handed back would be gone for good.
    pub async fn run(
        &self,
        run_id: Option<String>,
        state_name: impl Into<String>,
        events: Vec<Signal>,
        session: T,
    ) -> Result<Context<T>, RunError> {
        let run_id = run_id.or_else(|| Some(generate_run_id()));
        let mut ctx = Context::new(state_name, session, run_id);

        self.check_always(&mut ctx).await?;
        for signal in &events {
            self.push_signal(&mut ctx, signal).await?;
        }

        Ok(ctx)
    }

    /// Dispatch one signal. If it fires an `on`-transition, resolve pending `always`-transitions
    /// afterward. `error_state` covers the whole transition attempt -- the firing candidate's
    /// own actions *and* the `entry`/`exit` actions of the state it enters -- keyed on the state
    /// active when the signal was dispatched (entering the recovery state counts as "fired" too).
    /// `always`-transition failures (from `check_always`, called separately below) are
    /// deliberately not covered; see [`RunError`]'s docs for why.
    async fn push_signal(&self, ctx: &mut Context<T>, signal: &Signal) -> Result<bool, RunError> {
        let current = ctx.current_state.clone();
        let transitions = self
            .transition_table
            .get(&(current.clone(), signal.event.clone()))
            .or_else(|| self.transition_table.get(&(current.clone(), "*".to_string())))
            .cloned();

        let Some(transitions) = transitions else {
            return Ok(false);
        };

        let fired = match self.try_transitions(ctx, &transitions, signal).await {
            Ok(fired) => fired,
            Err(RunError::ActionFailed { state, action, source }) => {
                let error_state = self.config.get(&current).and_then(|cfg| cfg.error_state.clone());
                match error_state {
                    Some(error_state) => {
                        self.enter(ctx, &error_state, signal).await?;
                        true
                    }
                    None => return Err(RunError::ActionFailed { state, action, source }),
                }
            }
            Err(other) => return Err(other),
        };

        if fired {
            self.check_always(ctx).await?;
        }
        Ok(fired)
    }

    /// Resolve the current state's `always` candidates, restarting from the new state after each
    /// hop, until none pass. Capped at [`ALWAYS_MAX_DEPTH`] hops.
    async fn check_always(&self, ctx: &mut Context<T>) -> Result<(), RunError> {
        let always_signal = Signal::new("__always__");

        for _ in 0..ALWAYS_MAX_DEPTH {
            let current = ctx.current_state.clone();
            let always = match self.config.get(&current) {
                Some(cfg) if !cfg.always.is_empty() => cfg.always.clone(),
                _ => return Ok(()),
            };

            let mut fired = false;
            for candidate in &always {
                let passed = self.guards.evaluate(candidate.guard.as_deref(), ctx, &always_signal, HookSource::Always).await?;
                if passed {
                    self.actions.execute_many(&candidate.actions, ctx, &always_signal, HookSource::Always).await?;
                    self.enter(ctx, &candidate.target, &always_signal).await?;
                    fired = true;
                    break;
                }
            }
            if !fired {
                return Ok(());
            }
        }

        Err(RunError::AlwaysLoopExceeded(ctx.current_state.clone()))
    }

    /// Both this candidate's own `actions` and the `enter()` call's `entry`/`exit` actions can
    /// produce `RunError::ActionFailed` here -- `push_signal` treats them the same way (both
    /// eligible for `error_state` recovery), so no special-casing is needed between the two.
    async fn try_transitions(&self, ctx: &mut Context<T>, transitions: &[TransitionConfig], signal: &Signal) -> Result<bool, RunError> {
        for candidate in transitions {
            let passed = self.guards.evaluate(candidate.guard.as_deref(), ctx, signal, HookSource::On).await?;
            if passed {
                self.actions.execute_many(&candidate.actions, ctx, signal, HookSource::On).await?;
                self.enter(ctx, &candidate.target, signal).await?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn enter(&self, ctx: &mut Context<T>, state_name: &str, signal: &Signal) -> Result<(), RunError> {
        let current = ctx.current_state.clone();
        if let Some(exit_actions) = self.config.get(&current).map(|cfg| cfg.exit.clone()) {
            self.actions.execute_many(&exit_actions, ctx, signal, HookSource::Exit).await?;
        }

        ctx.history.push(state_name.to_string());
        ctx.current_state = state_name.to_string();

        if let Some(entry_actions) = self.config.get(state_name).map(|cfg| cfg.entry.clone()) {
            self.actions.execute_many(&entry_actions, ctx, signal, HookSource::Entry).await?;
        }

        Ok(())
    }
}
