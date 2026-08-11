//! The state machine engine.
//!
//! Mirrors `StateMachine` in `statem/machine.py`: config-driven `on`-transitions (with guard
//! chains), `entry`/`exit` actions, the `always`-transition auto-advance cascade, `error_state`
//! recovery, `validate_registries`, and Mermaid diagram export via [`StateMachine::diagram`].

use std::collections::HashMap;

use indexmap::IndexMap;

#[cfg(feature = "agui")]
use crate::agui;
use crate::diagram::Diagram;
use crate::registry::{Action, ActionRegistry, Guard, GuardRegistry};
use crate::schema::{generate_run_id, BuildError, Context, HookSource, ResultEntry, RunError, Signal, StateConfig, TransitionConfig, ValidationError};

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
    /// `run_id` is an optional correlation id; a fresh one is generated when omitted, same as
    /// `thread_id` (a correlation id for the broader conversation/session this run belongs to --
    /// one thread can span many runs). `events` may be empty to only resolve pending
    /// `always`-transitions for the current state.
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
        thread_id: Option<String>,
        state_name: impl Into<String>,
        events: Vec<Signal>,
        session: T,
    ) -> Result<Context<T>, RunError> {
        let run_id = run_id.or_else(|| Some(generate_run_id()));
        let thread_id = thread_id.or_else(|| Some(generate_run_id()));
        let mut ctx = Context::new(state_name, session, run_id, thread_id);

        self.check_always(&mut ctx).await?;
        for signal in &events {
            self.push_signal(&mut ctx, signal).await?;
        }

        Ok(ctx)
    }

    /// Real step/activity emission, used when the `agui` feature is enabled. Each helper is a
    /// no-op whenever `ctx` wasn't created by [`StateMachine::stream`] (i.e. for every `run()`
    /// call) -- see [`Context::is_stream`].
    #[cfg(feature = "agui")]
    fn current_state_dict(&self, ctx: &Context<T>) -> serde_json::Value {
        match &ctx.state_accessor {
            Some(accessor) => accessor(&ctx.session),
            None => serde_json::json!({ "current_state": ctx.current_state }),
        }
    }

    #[cfg(feature = "agui")]
    fn emit(&self, ctx: &Context<T>, event: agui::AguiEvent) {
        if let Some(tx) = &ctx.event_tx {
            let _ = tx.unbounded_send(event);
        }
    }

    /// Open one AG-UI step: `STEP_STARTED` followed immediately by a `STATE_SNAPSHOT`. Resets
    /// `ctx.state_snapshot` to the state *right now* (before this hop's guard/actions run) so
    /// [`StateMachine::close_step`] can diff against it -- each step's `STATE_DELTA` reflects
    /// only what changed during that step, not the whole stream.
    #[cfg(feature = "agui")]
    fn open_step(&self, ctx: &mut Context<T>, step_name: &str) {
        if !ctx.is_stream() {
            return;
        }
        self.emit(ctx, agui::step_started(step_name));
        let snapshot = self.current_state_dict(ctx);
        ctx.state_snapshot = Some(snapshot.clone());
        self.emit(ctx, agui::state_snapshot(snapshot));
    }

    /// Close one AG-UI step: `STATE_DELTA` (if anything changed) then `STEP_FINISHED`.
    #[cfg(feature = "agui")]
    fn close_step(&self, ctx: &mut Context<T>, step_name: &str) {
        if !ctx.is_stream() {
            return;
        }
        let new_state = self.current_state_dict(ctx);
        let old_state = ctx.state_snapshot.take().unwrap_or_else(|| serde_json::json!({}));
        let patch = agui::diff_state(&old_state, &new_state);
        ctx.state_snapshot = Some(new_state);
        if patch.as_array().is_some_and(|ops| !ops.is_empty()) {
            self.emit(ctx, agui::state_delta(patch));
        }
        self.emit(ctx, agui::step_finished(step_name));
    }

    #[cfg(feature = "agui")]
    fn emit_activities(&self, ctx: &Context<T>, entries: &[ResultEntry]) {
        if !ctx.is_stream() {
            return;
        }
        for entry in entries {
            self.emit(ctx, agui::activity(entry));
        }
    }

    #[cfg(not(feature = "agui"))]
    fn open_step(&self, _ctx: &mut Context<T>, _step_name: &str) {}

    #[cfg(not(feature = "agui"))]
    fn close_step(&self, _ctx: &mut Context<T>, _step_name: &str) {}

    #[cfg(not(feature = "agui"))]
    fn emit_activities(&self, _ctx: &Context<T>, _entries: &[ResultEntry]) {}

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
                        self.open_step(ctx, &signal.event);
                        self.enter(ctx, &error_state, signal).await?;
                        self.close_step(ctx, &signal.event);
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

            self.open_step(ctx, &always_signal.event);
            let mut fired = false;
            for candidate in &always {
                let passed = self.guards.evaluate(candidate.guard.as_deref(), ctx, &always_signal, HookSource::Always).await?;
                if candidate.guard.is_some() {
                    if let Some(entry) = ctx.results.last().cloned() {
                        self.emit_activities(ctx, std::slice::from_ref(&entry));
                    }
                }
                if passed {
                    let n_before = ctx.results.len();
                    self.actions.execute_many(&candidate.actions, ctx, &always_signal, HookSource::Always).await?;
                    let fired_entries: Vec<ResultEntry> = ctx.results[n_before..].to_vec();
                    self.emit_activities(ctx, &fired_entries);
                    self.enter(ctx, &candidate.target, &always_signal).await?;
                    fired = true;
                    break;
                }
            }
            self.close_step(ctx, &always_signal.event);
            if !fired {
                return Ok(());
            }
        }

        Err(RunError::AlwaysLoopExceeded(ctx.current_state.clone()))
    }

    /// Both this candidate's own `actions` and the `enter()` call's `entry`/`exit` actions can
    /// produce `RunError::ActionFailed` here -- `push_signal` treats them the same way (both
    /// eligible for `error_state` recovery), so no special-casing is needed between the two.
    ///
    /// Opens one AG-UI step for the whole candidate-resolution round (not per candidate) and
    /// always closes it before returning -- including on error -- by delegating to
    /// [`StateMachine::try_transitions_inner`] and closing the step around its result, the
    /// closest Rust equivalent of Python's `try`/`finally`.
    async fn try_transitions(&self, ctx: &mut Context<T>, transitions: &[TransitionConfig], signal: &Signal) -> Result<bool, RunError> {
        self.open_step(ctx, &signal.event);
        let result = self.try_transitions_inner(ctx, transitions, signal).await;
        self.close_step(ctx, &signal.event);
        result
    }

    async fn try_transitions_inner(&self, ctx: &mut Context<T>, transitions: &[TransitionConfig], signal: &Signal) -> Result<bool, RunError> {
        for candidate in transitions {
            let passed = self.guards.evaluate(candidate.guard.as_deref(), ctx, signal, HookSource::On).await?;
            if candidate.guard.is_some() {
                if let Some(entry) = ctx.results.last().cloned() {
                    self.emit_activities(ctx, std::slice::from_ref(&entry));
                }
            }
            if passed {
                let n_before = ctx.results.len();
                self.actions.execute_many(&candidate.actions, ctx, signal, HookSource::On).await?;
                let fired_entries: Vec<ResultEntry> = ctx.results[n_before..].to_vec();
                self.emit_activities(ctx, &fired_entries);
                self.enter(ctx, &candidate.target, signal).await?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn enter(&self, ctx: &mut Context<T>, state_name: &str, signal: &Signal) -> Result<(), RunError> {
        let current = ctx.current_state.clone();
        if let Some(exit_actions) = self.config.get(&current).map(|cfg| cfg.exit.clone()) {
            if !exit_actions.is_empty() {
                let n_before = ctx.results.len();
                self.actions.execute_many(&exit_actions, ctx, signal, HookSource::Exit).await?;
                let fired_entries: Vec<ResultEntry> = ctx.results[n_before..].to_vec();
                self.emit_activities(ctx, &fired_entries);
            }
        }

        ctx.history.push(state_name.to_string());
        ctx.current_state = state_name.to_string();

        if let Some(entry_actions) = self.config.get(state_name).map(|cfg| cfg.entry.clone()) {
            if !entry_actions.is_empty() {
                let n_before = ctx.results.len();
                self.actions.execute_many(&entry_actions, ctx, signal, HookSource::Entry).await?;
                let fired_entries: Vec<ResultEntry> = ctx.results[n_before..].to_vec();
                self.emit_activities(ctx, &fired_entries);
            }
        }

        Ok(())
    }
}

#[cfg(feature = "agui")]
impl<T: Send + Sync> StateMachine<T> {
    /// Like [`StateMachine::run`], but returns a stream of AG-UI protocol events as the machine
    /// executes, plus a [`futures_channel::oneshot::Receiver`] that resolves to the final
    /// [`Context<T>`] (mutated `session`, `history`, `results`) once the stream is fully drained.
    /// The receiver exists for the same reason `run()` returns the whole `Context<T>` instead of
    /// just the final state name: `session` is moved in by value, and Rust's ownership model has
    /// no other way to hand it back to the caller. Requires the `agui` Cargo feature.
    ///
    /// Drives the same engine as `run` (same guards, actions, transition rules) and has the same
    /// effect on `session` -- this is an additive, alternate way to observe a run, not a
    /// different execution path.
    ///
    /// A step = one state change (one hop), whether triggered by an `on` transition, an `always`
    /// cascade hop, or an `error_state` fallback -- not one call to `stream()`. Each step is
    /// fully self-contained, emitted in this order: `STEP_STARTED` (`step_name` is the
    /// triggering signal's event, or `"__always__"` for an `always`-cascade hop) -> a
    /// `STATE_SNAPSHOT` taken right before this hop's guards/actions run -> an
    /// `ACTIVITY_SNAPSHOT` for every guard/action result as it fires during this hop (a guard
    /// evaluated but not taken is reported the same way, before its step -- or the step before
    /// it -- opens) -> a `STATE_DELTA` (RFC 6902 patch against this step's own `STATE_SNAPSHOT`,
    /// skipped if nothing changed) -> `STEP_FINISHED`.
    ///
    /// A signal matching no transition, or whose guard(s) all fail, produces no events (no state
    /// change happened). `RunStarted`/`RunFinished`/`RunError` AG-UI events are never emitted --
    /// an engine error surfaces as `Err(RunError)` from the stream itself, exactly as it would
    /// from `run`.
    ///
    /// Unlike `run`, which never awaits anything beyond the guard/action futures it's handed and
    /// so works under any executor, `stream()` uses `futures_util::select!` internally to
    /// interleave engine progress with event delivery -- it does not spawn onto any specific
    /// runtime, so it stays just as executor-agnostic as `run()`.
    ///
    /// `state_accessor` derives the value broadcast via `STATE_SNAPSHOT`/`STATE_DELTA` from
    /// `session` (e.g. rendering it to `serde_json::Value`); pass `None` to default to
    /// `{"current_state": <state name>}`.
    #[allow(clippy::too_many_arguments)]
    pub fn stream(
        &self,
        run_id: Option<String>,
        thread_id: Option<String>,
        state_name: impl Into<String>,
        events: Vec<Signal>,
        session: T,
        state_accessor: Option<crate::schema::StateAccessor<T>>,
    ) -> (impl futures_util::Stream<Item = Result<agui::AguiEvent, RunError>> + '_, futures_channel::oneshot::Receiver<Context<T>>) {
        let run_id = run_id.or_else(|| Some(generate_run_id()));
        let thread_id = thread_id.or_else(|| Some(generate_run_id()));
        let state_name = state_name.into();
        let (ctx_tx, ctx_rx) = futures_channel::oneshot::channel();

        let events_stream = async_stream::stream! {
            use futures_util::FutureExt as _;
            use futures_util::StreamExt as _;

            let (tx, mut rx) = futures_channel::mpsc::unbounded();
            let mut ctx = Context::new(state_name, session, run_id, thread_id);
            ctx.event_tx = Some(tx);
            ctx.state_accessor = state_accessor;

            // `drive` borrows `ctx` mutably for as long as it's alive, so it's scoped to this
            // inner block -- it must be fully dropped (releasing that borrow) before `ctx` can
            // be moved into `ctx_tx.send(ctx)` below.
            let run_error = {
                let drive = async {
                    self.check_always(&mut ctx).await?;
                    for signal in &events {
                        self.push_signal(&mut ctx, signal).await?;
                    }
                    Ok::<(), RunError>(())
                }
                .fuse();
                futures_util::pin_mut!(drive);

                loop {
                    futures_util::select! {
                        result = drive => break result.err(),
                        ev = rx.next().fuse() => {
                            match ev {
                                Some(ev) => yield Ok(ev),
                                None => break None,
                            }
                        }
                    }
                }
            };

            while let Ok(ev) = rx.try_recv() {
                yield Ok(ev);
            }

            let _ = ctx_tx.send(ctx);
            if let Some(err) = run_error {
                yield Err(err);
            }
        };

        (events_stream, ctx_rx)
    }
}
