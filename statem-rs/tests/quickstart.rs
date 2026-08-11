//! Rust counterpart to the Python quickstart (`docs/quickstart.md`): `idle -> running` via a
//! single guarded `on`-transition. Proves the foundational engine (config parsing, guard
//! registration/evaluation, `on`-transition dispatch, `run()`) works end-to-end.

use indexmap::IndexMap;
use statem_rs::{Context, HookSource, ResultKind, Signal, StateConfig, StateMachine};

fn can_start<T>(_ctx: &Context<T>, _signal: &Signal) -> bool {
    true
}

fn parse(json: &str) -> IndexMap<String, StateConfig> {
    serde_json::from_str(json).expect("config should deserialize")
}

#[tokio::test]
async fn idle_to_running_via_guarded_transition() {
    let config = parse(
        r#"{
            "idle": {"on": {"START": {"target": "running", "guard": "can_start"}}},
            "running": {"on": {"STOP": "idle"}}
        }"#,
    );

    let mut machine = StateMachine::<()>::from_config(config).expect("targets should be valid");
    machine.register_guard("can_start", can_start);

    let ctx = machine
        .run(None, None, "idle", vec![Signal::new("START")], ())
        .await
        .expect("run should succeed");

    assert_eq!(ctx.current_state, "running");
}

/// Regression test for a real gap `Context::trace()` (see `tests/trace.rs`) never caught: that
/// module only ever exercised `Trace`'s `Display` impl against a hand-built `Context`, so a
/// version of the engine that silently never recorded any `ResultEntry` during a live `run()`
/// still passed every test. Drive a real run through the public API and check
/// `ctx.results`/`ctx.trace()` on the `Context` `run()` hands back.
#[tokio::test]
async fn run_records_guard_and_action_results_for_trace() {
    let config = parse(
        r#"{
            "idle": {"on": {"START": {"target": "running", "guard": "can_start", "actions": ["log_start"]}}},
            "running": {"entry": ["log_entry"]}
        }"#,
    );

    let mut machine = StateMachine::<()>::from_config(config).expect("targets should be valid");
    machine.register_guard("can_start", can_start);
    machine.register_action("log_start", |_ctx: &mut Context<()>, _signal: &Signal| Ok(()));
    machine.register_action("log_entry", |_ctx: &mut Context<()>, _signal: &Signal| Ok(()));

    let ctx = machine
        .run(None, None, "idle", vec![Signal::new("START")], ())
        .await
        .expect("run should succeed");

    let guard_entries: Vec<_> = ctx.results.iter().filter(|e| matches!(e.kind, ResultKind::Guard(_))).collect();
    let action_entries: Vec<_> = ctx.results.iter().filter(|e| matches!(e.kind, ResultKind::Action)).collect();

    assert_eq!(guard_entries.len(), 1);
    assert_eq!(guard_entries[0].name, "can_start");
    assert_eq!(guard_entries[0].source, HookSource::On);

    assert_eq!(action_entries.len(), 2);
    assert_eq!(action_entries[0].name, "log_start");
    assert_eq!(action_entries[0].source, HookSource::On);
    assert_eq!(action_entries[1].name, "log_entry");
    assert_eq!(action_entries[1].source, HookSource::Entry);

    let trace_text = ctx.trace().to_string();
    assert!(trace_text.contains("can_start"));
    assert!(trace_text.contains("log_start"));
    assert!(trace_text.contains("log_entry"));
}
