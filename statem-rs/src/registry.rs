//! Guard/action traits and their registries.
//!
//! Mirrors `ActionRegistry`/`GuardRegistry` in `statem/schema.py`. Guards receive `&Context<T>`
//! (read-only); actions receive `&mut Context<T>` -- enforced by the borrow checker, a
//! deliberate improvement over the Python port (see the crate root's design notes).
//!
//! Plain **synchronous** closures/functions get a blanket impl, so the common case doesn't need
//! a struct + trait impl (mirrors how trivially Python registers a bare function). Native async
//! closures aren't stable on this crate's MSRV (1.82; stabilized in 1.85), so an async
//! guard/action needs a one-line `#[async_trait::async_trait] impl Guard<T> for MyGuard { .. }`
//! for now -- still just one extra `impl` block, not a real ergonomics cliff.

use std::collections::HashMap;

use async_trait::async_trait;

use crate::schema::{Context, HookSource, ResultEntry, ResultKind, RunError, Signal};

/// A named, registered predicate that gates a transition candidate. Must return `bool` --
/// unlike the Python port, there's no `GuardError` ("returned a non-bool") because the type
/// system makes that case impossible to construct.
#[async_trait]
pub trait Guard<T>: Send + Sync {
    async fn evaluate(&self, ctx: &Context<T>, signal: &Signal) -> bool;
}

#[async_trait]
impl<T, F> Guard<T> for F
where
    T: Send + Sync,
    F: Fn(&Context<T>, &Signal) -> bool + Send + Sync,
{
    async fn evaluate(&self, ctx: &Context<T>, signal: &Signal) -> bool {
        self(ctx, signal)
    }
}

/// A named, registered side effect run on entry/exit/transition.
#[async_trait]
pub trait Action<T>: Send + Sync {
    async fn execute(&self, ctx: &mut Context<T>, signal: &Signal) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

#[async_trait]
impl<T, F> Action<T> for F
where
    T: Send + Sync,
    F: Fn(&mut Context<T>, &Signal) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync,
{
    async fn execute(&self, ctx: &mut Context<T>, signal: &Signal) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self(ctx, signal)
    }
}

/// Registry of named guard functions.
pub struct GuardRegistry<T> {
    guards: HashMap<String, Box<dyn Guard<T>>>,
}

impl<T> Default for GuardRegistry<T> {
    fn default() -> Self {
        GuardRegistry { guards: HashMap::new() }
    }
}

impl<T> GuardRegistry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, guard: impl Guard<T> + 'static) {
        self.guards.insert(name.into(), Box::new(guard));
    }

    pub fn has(&self, name: &str) -> bool {
        self.guards.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.guards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.guards.is_empty()
    }

    /// Evaluate a guard by name, recording a [`ResultEntry`] on `ctx`. `None` means "no guard" ->
    /// always passes, and records nothing (there's no named guard to attribute the pass to).
    pub async fn evaluate(&self, name: Option<&str>, ctx: &mut Context<T>, signal: &Signal, source: HookSource) -> Result<bool, RunError> {
        let Some(name) = name else {
            return Ok(true);
        };
        let guard = self.guards.get(name).ok_or_else(|| RunError::UnknownGuard(name.to_string()))?;
        let passed = guard.evaluate(ctx, signal).await;
        ctx.results.push(ResultEntry { state: ctx.current_state.clone(), source, name: name.to_string(), kind: ResultKind::Guard(passed) });
        Ok(passed)
    }
}

/// Registry of named action functions.
pub struct ActionRegistry<T> {
    actions: HashMap<String, Box<dyn Action<T>>>,
}

impl<T> Default for ActionRegistry<T> {
    fn default() -> Self {
        ActionRegistry { actions: HashMap::new() }
    }
}

impl<T> ActionRegistry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, action: impl Action<T> + 'static) {
        self.actions.insert(name.into(), Box::new(action));
    }

    pub fn has(&self, name: &str) -> bool {
        self.actions.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Run a single named action, recording a [`ResultEntry`] on `ctx` once it succeeds. A
    /// failing action records nothing -- it aborts via `?` before the push, same as the guard
    /// path never recording a failed lookup.
    pub async fn execute(&self, name: &str, ctx: &mut Context<T>, signal: &Signal, source: HookSource) -> Result<(), RunError> {
        let action = self.actions.get(name).ok_or_else(|| RunError::UnknownAction(name.to_string()))?;
        action
            .execute(ctx, signal)
            .await
            .map_err(|source| RunError::ActionFailed { state: ctx.current_state.clone(), action: name.to_string(), source })?;
        ctx.results.push(ResultEntry { state: ctx.current_state.clone(), source, name: name.to_string(), kind: ResultKind::Action });
        Ok(())
    }

    /// Run actions in order, awaiting each one.
    pub async fn execute_many(&self, names: &[String], ctx: &mut Context<T>, signal: &Signal, source: HookSource) -> Result<(), RunError> {
        for name in names {
            self.execute(name, ctx, signal, source).await?;
        }
        Ok(())
    }
}
