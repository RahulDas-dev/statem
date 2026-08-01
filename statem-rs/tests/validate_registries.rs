//! Covers `StateMachine::validate_registries`, mirroring `TestFromDict`'s guard/action-lookup
//! cases in `tests/test_machine.py` on the Python side. Unlike bad transition targets (checked
//! eagerly by `from_config`), guard/action registration typically happens *after* construction,
//! so `validate_registries` is a separate, explicit call.

use indexmap::IndexMap;
use statem_rs::{Context, Signal, StateConfig, StateMachine};

fn guard_true(_ctx: &Context<()>, _signal: &Signal) -> bool {
    true
}

fn action_ok(_ctx: &mut Context<()>, _signal: &Signal) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Ok(())
}

fn parse(json: &str) -> IndexMap<String, StateConfig> {
    serde_json::from_str(json).expect("config should deserialize")
}

#[test]
fn all_registered_passes() {
    let config = parse(
        r#"{
            "idle": {
                "on": {"START": {"target": "running", "guard": "can_start", "actions": ["log_start"]}},
                "entry": ["on_entry"],
                "exit": ["on_exit"]
            },
            "running": {"always": [{"target": "idle", "guard": "should_stop", "actions": ["log_stop"]}]}
        }"#,
    );
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_guard("can_start", guard_true);
    machine.register_guard("should_stop", guard_true);
    machine.register_action("log_start", action_ok);
    machine.register_action("log_stop", action_ok);
    machine.register_action("on_entry", action_ok);
    machine.register_action("on_exit", action_ok);

    assert!(machine.validate_registries().is_ok());
}

#[test]
fn missing_entry_action_reported() {
    let config = parse(r#"{"idle": {"entry": ["ghost_entry"]}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();

    let err = machine.validate_registries().unwrap_err();
    assert!(err.missing_actions.iter().any(|m| m == "idle.entry: ghost_entry"), "{err:?}");
}

#[test]
fn missing_exit_action_reported() {
    let config = parse(r#"{"idle": {"exit": ["ghost_exit"]}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();

    let err = machine.validate_registries().unwrap_err();
    assert!(err.missing_actions.iter().any(|m| m == "idle.exit: ghost_exit"), "{err:?}");
}

#[test]
fn missing_on_guard_reported() {
    let config = parse(r#"{"idle": {"on": {"START": {"target": "running", "guard": "ghost_guard"}}}, "running": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();

    let err = machine.validate_registries().unwrap_err();
    assert!(err.missing_guards.iter().any(|m| m == "idle.on.START: ghost_guard"), "{err:?}");
}

#[test]
fn missing_on_action_reported() {
    let config = parse(r#"{"idle": {"on": {"START": {"target": "running", "actions": ["ghost_action"]}}}, "running": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();

    let err = machine.validate_registries().unwrap_err();
    assert!(err.missing_actions.iter().any(|m| m == "idle.on.START: ghost_action"), "{err:?}");
}

#[test]
fn missing_always_guard_and_action_both_reported() {
    let config = parse(r#"{"idle": {"always": [{"target": "idle", "guard": "ghost_guard", "actions": ["ghost_action"]}]}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();

    let err = machine.validate_registries().unwrap_err();
    assert!(err.missing_guards.iter().any(|m| m == "idle.always[0]: ghost_guard"), "{err:?}");
    assert!(err.missing_actions.iter().any(|m| m == "idle.always[0]: ghost_action"), "{err:?}");
}
