//! Mermaid `stateDiagram-v2` export.
//!
//! Rendering only ever needs `config` (state names, `on`/`always`/`error_state`) -- never the
//! session type `T`, since guards/actions/session don't affect the static graph shape. So
//! [`Diagram`] borrows `&IndexMap<String, StateConfig>` directly rather than `&StateMachine<T>`,
//! keeping it non-generic instead of leaking a type parameter it has nothing to do with.
//!
//! Like [`crate::tracing::Trace`], this implements `Display` rather than returning an eager
//! `String` -- composes with `println!`/`write!`/`.to_string()` without forcing an allocation.

use std::collections::HashSet;
use std::fmt;

use indexmap::IndexMap;

use crate::schema::StateConfig;

/// Renders as Mermaid `stateDiagram-v2` source, e.g.:
///
/// ```text
/// stateDiagram-v2
///     [*] --> idle
///     idle --> running: START [can_start]
///     running --> idle: STOP
/// ```
///
/// One edge per transition candidate: `on` candidates are labeled with the event name (plus
/// `[guard]` if guarded), `always` candidates are labeled `always` (plus the guard, if any), and
/// each `error_state` becomes an `error`-labeled edge. State names that aren't already safe
/// Mermaid identifiers get a `state "name" as safe_id` alias line and their edges reference the
/// alias; two names that sanitize to the same id get a numeric suffix to stay distinct.
pub struct Diagram<'a> {
    config: &'a IndexMap<String, StateConfig>,
    ids: IndexMap<String, String>,
    initial: Option<String>,
}

impl<'a> Diagram<'a> {
    pub(crate) fn new(config: &'a IndexMap<String, StateConfig>, initial: Option<&str>) -> Self {
        let ids = assign_ids(config);
        let initial = initial.filter(|name| config.contains_key(*name)).map(String::from);
        Diagram { config, ids, initial }
    }
}

impl fmt::Display for Diagram<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "stateDiagram-v2")?;

        for (name, id) in &self.ids {
            if id != name {
                writeln!(f, "    state \"{name}\" as {id}")?;
            }
        }

        if let Some(initial) = &self.initial {
            writeln!(f, "    [*] --> {}", self.ids[initial])?;
        }

        for (name, state_cfg) in self.config {
            let src = &self.ids[name];

            for (event, candidates) in &state_cfg.on {
                for candidate in candidates {
                    let label = match &candidate.guard {
                        Some(guard) => format!("{event} [{guard}]"),
                        None => event.clone(),
                    };
                    writeln!(f, "    {src} --> {}: {label}", self.ids[&candidate.target])?;
                }
            }

            for candidate in &state_cfg.always {
                let label = match &candidate.guard {
                    Some(guard) => format!("always [{guard}]"),
                    None => "always".to_string(),
                };
                writeln!(f, "    {src} --> {}: {label}", self.ids[&candidate.target])?;
            }

            if let Some(error_state) = &state_cfg.error_state {
                writeln!(f, "    {src} --> {}: error", self.ids[error_state])?;
            }
        }

        Ok(())
    }
}

/// Map each state name to a safe, unique Mermaid id, preserving declaration order (relies on
/// `config` being an [`IndexMap`] -- see [`StateConfig::on`]'s docs for why that matters here).
fn assign_ids(config: &IndexMap<String, StateConfig>) -> IndexMap<String, String> {
    let mut ids = IndexMap::new();
    let mut used: HashSet<String> = HashSet::new();

    for name in config.keys() {
        let mut safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
        if safe.is_empty() || safe.starts_with(|c: char| c.is_ascii_digit()) {
            safe = format!("s_{safe}");
        }

        let mut candidate = safe.clone();
        let mut suffix = 2;
        while used.contains(&candidate) {
            candidate = format!("{safe}_{suffix}");
            suffix += 1;
        }

        used.insert(candidate.clone());
        ids.insert(name.clone(), candidate);
    }

    ids
}
