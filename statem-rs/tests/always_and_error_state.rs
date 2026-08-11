//! Covers the `always`-cascade and `error_state` recovery. `error_state` covers the whole
//! `on`-transition attempt (the firing candidate's own `actions` *and* the `entry`/`exit`
//! actions of the state it then enters), but deliberately never covers an `always`-transition's
//! actions -- see [`statem_rs::RunError`]'s docs for why.

use std::error::Error;

use indexmap::IndexMap;
use statem_rs::{Context, RunError, Signal, StateConfig, StateMachine};

fn guard_true(_ctx: &Context<()>, _signal: &Signal) -> bool {
    true
}

fn guard_false(_ctx: &Context<()>, _signal: &Signal) -> bool {
    false
}

fn action_fails(_ctx: &mut Context<()>, _signal: &Signal) -> Result<(), Box<dyn Error + Send + Sync>> {
    Err("boom".into())
}

fn parse(json: &str) -> IndexMap<String, StateConfig> {
    serde_json::from_str(json).expect("config should deserialize")
}

#[tokio::test]
async fn always_with_no_guard_passing_leaves_state_unchanged() {
    let config = parse(r#"{"idle": {"always": [{"target": "running", "guard": "never"}]}, "running": {}}"#);
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_guard("never", guard_false);

    let ctx = machine.run(None, None, "idle", vec![], ()).await.unwrap();
    assert_eq!(ctx.current_state, "idle");
}

#[tokio::test]
async fn always_chain_restarts_from_each_new_state() {
    let config = parse(r#"{"a": {"always": [{"target": "b"}]}, "b": {"always": [{"target": "c"}]}, "c": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();

    let ctx = machine.run(None, None, "a", vec![], ()).await.unwrap();
    assert_eq!(ctx.current_state, "c");
}

#[tokio::test]
async fn always_loop_that_never_settles_errors() {
    let config = parse(r#"{"a": {"always": [{"target": "b"}]}, "b": {"always": [{"target": "a"}]}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();

    let err = machine.run(None, None, "a", vec![], ()).await.unwrap_err();
    assert!(matches!(err, RunError::AlwaysLoopExceeded(_)), "expected AlwaysLoopExceeded, got {err:?}");
}

#[tokio::test]
async fn on_transition_action_failure_falls_back_to_error_state() {
    let config = parse(
        r#"{
            "idle": {"on": {"START": {"target": "running", "actions": ["boom"]}}, "error_state": "failed"},
            "running": {},
            "failed": {}
        }"#,
    );
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_action("boom", action_fails);

    let ctx = machine.run(None, None, "idle", vec![Signal::new("START")], ()).await.unwrap();
    assert_eq!(ctx.current_state, "failed");
}

#[tokio::test]
async fn on_transition_action_failure_without_error_state_propagates() {
    let config = parse(r#"{"idle": {"on": {"START": {"target": "running", "actions": ["boom"]}}}, "running": {}}"#);
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_action("boom", action_fails);

    let err = machine.run(None, None, "idle", vec![Signal::new("START")], ()).await.unwrap_err();
    assert!(matches!(err, RunError::ActionFailed { .. }), "expected ActionFailed, got {err:?}");
}

#[tokio::test]
async fn entry_action_failure_is_caught_by_error_state() {
    // "idle" declares an error_state; the failing action is "running"'s *entry* action, not
    // "idle"'s on-transition action -- error_state covers this too (see the module docs: it
    // covers the whole transition attempt, not just the firing candidate's own actions).
    let config = parse(
        r#"{
            "idle": {"on": {"START": "running"}, "error_state": "failed"},
            "running": {"entry": ["boom"]},
            "failed": {}
        }"#,
    );
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_action("boom", action_fails);

    let ctx = machine.run(None, None, "idle", vec![Signal::new("START")], ()).await.unwrap();
    assert_eq!(ctx.current_state, "failed");
}

#[tokio::test]
async fn always_transition_action_failure_is_not_caught_by_error_state() {
    // Deliberately excluded (not just "matches Python"): `always` is a structural, continuously
    // re-checked invariant rather than a one-shot response to a signal, so a failure here is a
    // config/logic bug worth surfacing loudly, not a transient failure to route around.
    let config = parse(
        r#"{
            "idle": {"on": {"START": "running"}, "error_state": "failed"},
            "running": {"always": [{"target": "done", "actions": ["boom"]}]},
            "done": {},
            "failed": {}
        }"#,
    );
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_action("boom", action_fails);

    let err = machine.run(None, None, "idle", vec![Signal::new("START")], ()).await.unwrap_err();
    assert!(matches!(err, RunError::ActionFailed { .. }), "expected raw ActionFailed, got {err:?}");
}

#[tokio::test]
async fn guard_chain_still_works_alongside_always_and_error_state() {
    // Sanity check that Phase 1 behavior (guard chains) still holds after this phase's changes.
    let config = parse(
        r#"{
            "idle": {"on": {"GO": [{"target": "blocked", "guard": "no"}, {"target": "running", "guard": "yes"}]}},
            "blocked": {},
            "running": {}
        }"#,
    );
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_guard("no", guard_false);
    machine.register_guard("yes", guard_true);

    let ctx = machine.run(None, None, "idle", vec![Signal::new("GO")], ()).await.unwrap();
    assert_eq!(ctx.current_state, "running");
}
