//! Human-readable, hop-by-hop trace rendering for a [`Context`].
//!
//! The Python original (`show_transitions`) returns an eagerly-built `String` via manual
//! `string.Template` substitution, because that's the natural tool Python has for this. Rust's
//! natural tool is `std::fmt::Display`: [`Context::trace`] returns a lightweight, borrowing
//! [`Trace`] view that implements `Display`, so it composes directly with `println!`, `write!`,
//! logging macros, or `.to_string()` -- no forced allocation, no template-substitution machinery.

use std::fmt;

use crate::schema::{Context, HookSource, ResultEntry, ResultKind};

const DIVIDER: &str = "--------------------------------------------------------------------------------";
const SRC_WIDTH: usize = 6;
const NAME_WIDTH: usize = 25;

impl fmt::Display for HookSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            HookSource::On => "on",
            HookSource::Always => "always",
            HookSource::Entry => "entry",
            HookSource::Exit => "exit",
        })
    }
}

impl<T> Context<T> {
    /// A human-readable trace of every guard/action that fired during this run, grouped by hop.
    /// Borrows from `self` -- reads `history`/`results` only, never touches `session`.
    pub fn trace(&self) -> Trace<'_> {
        Trace { history: &self.history, results: &self.results }
    }
}

/// Renders as a hop-by-hop report, e.g.:
///
/// ```text
/// Transitions (1 hop):
/// --------------------------------------------------------------------------------
/// hop 1: idle -> running
///   on     guard  : can_start                 = true
///          action : create_txn
/// --------------------------------------------------------------------------------
/// Final state: running
/// --------------------------------------------------------------------------------
/// ```
///
/// Guard rows show their `bool` result; action rows don't show a value (this port doesn't
/// track action return values -- see [`ResultKind`]'s docs). Consecutive rows sharing the same
/// [`HookSource`] leave the source column blank after the first, matching the Python original.
pub struct Trace<'a> {
    history: &'a [String],
    results: &'a [ResultEntry],
}

impl fmt::Display for Trace<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hops = self.history.len().saturating_sub(1);
        writeln!(f, "Transitions ({hops} hop{}):", if hops == 1 { "" } else { "s" })?;
        writeln!(f, "{DIVIDER}")?;

        for (i, state) in self.history.iter().enumerate() {
            if i + 1 < self.history.len() {
                writeln!(f, "hop {}: {state} -> {}", i + 1, self.history[i + 1])?;
                self.write_state_section(f, state)?;
                writeln!(f, "{DIVIDER}")?;
            } else {
                writeln!(f, "Final state: {state}")?;
                self.write_state_section(f, state)?;
                write!(f, "{DIVIDER}")?;
            }
        }
        Ok(())
    }
}

impl Trace<'_> {
    fn write_state_section(&self, f: &mut fmt::Formatter<'_>, state: &str) -> fmt::Result {
        let mut prev_source: Option<HookSource> = None;
        for entry in self.results.iter().filter(|entry| entry.state == state) {
            let label = if prev_source != Some(entry.source) {
                format!("{:<SRC_WIDTH$}", entry.source.to_string())
            } else {
                " ".repeat(SRC_WIDTH)
            };
            prev_source = Some(entry.source);

            match entry.kind {
                ResultKind::Guard(value) => writeln!(f, "  {label} guard  : {:<NAME_WIDTH$} = {value}", entry.name)?,
                ResultKind::Action => writeln!(f, "  {label} action : {}", entry.name)?,
            }
        }
        Ok(())
    }
}
