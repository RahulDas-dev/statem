//! Covers `StateMachine::diagram()` / `Diagram`'s `Display` impl.

use indexmap::IndexMap;
use statem_rs::{StateConfig, StateMachine};

fn parse(json: &str) -> IndexMap<String, StateConfig> {
    serde_json::from_str(json).expect("config should deserialize")
}

#[test]
fn header_line() {
    let config = parse(r#"{"idle": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.starts_with("stateDiagram-v2\n"));
}

#[test]
fn on_edge_without_guard() {
    let config = parse(r#"{"idle": {"on": {"START": "running"}}, "running": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.contains("idle --> running: START"));
    assert!(!text.contains("START ["));
}

#[test]
fn on_edge_with_guard() {
    let config = parse(r#"{"idle": {"on": {"START": {"target": "running", "guard": "can_start"}}}, "running": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.contains("idle --> running: START [can_start]"));
}

#[test]
fn guard_chain_produces_multiple_labeled_edges() {
    let config = parse(
        r#"{
            "idle": {"on": {"GO": [
                {"target": "a", "guard": "g1"},
                {"target": "b", "guard": "g2"}
            ]}},
            "a": {}, "b": {}
        }"#,
    );
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.contains("idle --> a: GO [g1]"));
    assert!(text.contains("idle --> b: GO [g2]"));
}

#[test]
fn wildcard_event_is_a_literal_label() {
    let config = parse(r#"{"idle": {"on": {"*": "running"}}, "running": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.contains("idle --> running: *"));
}

#[test]
fn always_edge_without_and_with_guard() {
    let config = parse(
        r#"{
            "a": {"always": [{"target": "b"}]},
            "b": {"always": [{"target": "c", "guard": "ready"}]},
            "c": {}
        }"#,
    );
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.contains("a --> b: always"));
    assert!(!text.contains("a --> b: always ["));
    assert!(text.contains("b --> c: always [ready]"));
}

#[test]
fn error_state_edge() {
    let config = parse(r#"{"idle": {"error_state": "failed"}, "failed": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.contains("idle --> failed: error"));
}

#[test]
fn initial_present_prepends_entry_edge() {
    let config = parse(r#"{"idle": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(Some("idle")).to_string();

    assert!(text.contains("[*] --> idle"));
}

#[test]
fn initial_absent_or_unknown_omits_entry_edge() {
    let config = parse(r#"{"idle": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();

    assert!(!machine.diagram(None).to_string().contains("[*]"));
    assert!(!machine.diagram(Some("ghost")).to_string().contains("[*]"));
}

#[test]
fn safe_state_name_gets_no_alias_line() {
    let config = parse(r#"{"idle": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(!text.contains("state \"idle\" as"));
}

#[test]
fn unsafe_state_name_gets_aliased_and_referenced_by_id() {
    let config = parse(r#"{"my state": {"on": {"GO": "other-state"}}, "other-state": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.contains("state \"my state\" as my_state"));
    assert!(text.contains("state \"other-state\" as other_state"));
    assert!(text.contains("my_state --> other_state: GO"));
}

#[test]
fn colliding_sanitized_names_get_a_unique_suffix() {
    let config = parse(r#"{"a b": {"on": {"GO": "a-b"}}, "a-b": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.contains("state \"a b\" as a_b"));
    assert!(text.contains("state \"a-b\" as a_b_2"));
    assert!(text.contains("a_b --> a_b_2: GO"));
}

#[test]
fn leading_digit_state_name_gets_prefixed() {
    let config = parse(r#"{"1abc": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let text = machine.diagram(None).to_string();

    assert!(text.contains("state \"1abc\" as s_1abc"));
}

/// A Rust-specific regression test Python parity-checking would never have prompted: Python's
/// `dict` always preserves insertion order, so this failure mode (a `HashMap`'s iteration order
/// isn't guaranteed) couldn't happen there. Renders the same machine several times and requires
/// byte-for-byte identical output every time.
#[test]
fn diagram_output_is_deterministic_across_renders() {
    let config = parse(
        r#"{
            "idle": {"on": {"START_TXN": {"target": "a", "actions": ["x"]}}},
            "a": {"on": {"GO": [{"target": "b", "guard": "g1"}, {"target": "c", "guard": "g2"}]}, "error_state": "failed"},
            "b": {"always": [{"target": "c", "guard": "ready"}]},
            "c": {},
            "failed": {}
        }"#,
    );
    let machine = StateMachine::<()>::from_config(config).unwrap();

    let first = machine.diagram(Some("idle")).to_string();
    for _ in 0..20 {
        assert_eq!(machine.diagram(Some("idle")).to_string(), first);
    }
}
