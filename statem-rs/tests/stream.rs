//! Covers `StateMachine::stream()`: per-hop AG-UI event sequencing (`STEP_STARTED` ->
//! `STATE_SNAPSHOT` -> `ACTIVITY_SNAPSHOT`* -> `STATE_DELTA`? -> `STEP_FINISHED`), guard/action
//! activity content, the `state_accessor` override, and error propagation. Mirrors
//! `statem-py/tests/test_stream.py`'s scenarios.
//!
//! Whole file is gated on the `agui` feature (compiles to nothing otherwise), same as
//! `StateMachine::stream()` itself.
#![cfg(feature = "agui")]

use std::error::Error;
use std::sync::Arc;

use futures_util::StreamExt as _;
use indexmap::IndexMap;
use serde_json::json;
use statem_rs::{AguiEvent, Context, RunError, Signal, StateConfig, StateMachine};

fn guard_true(_ctx: &Context<()>, _signal: &Signal) -> bool {
    true
}

fn guard_false(_ctx: &Context<()>, _signal: &Signal) -> bool {
    false
}

fn no_op_action(_ctx: &mut Context<()>, _signal: &Signal) -> Result<(), Box<dyn Error + Send + Sync>> {
    Ok(())
}

fn action_fails(_ctx: &mut Context<()>, _signal: &Signal) -> Result<(), Box<dyn Error + Send + Sync>> {
    Err("boom".into())
}

fn bump_counter(ctx: &mut Context<i32>, _signal: &Signal) -> Result<(), Box<dyn Error + Send + Sync>> {
    ctx.session += 1;
    Ok(())
}

fn parse(json: &str) -> IndexMap<String, StateConfig> {
    serde_json::from_str(json).expect("config should deserialize")
}

async fn collect(stream: impl futures_util::Stream<Item = Result<AguiEvent, RunError>>) -> Result<Vec<AguiEvent>, RunError> {
    futures_util::pin_mut!(stream);
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item?);
    }
    Ok(events)
}

fn type_names(events: &[AguiEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match e {
            AguiEvent::StepStarted(_) => "STEP_STARTED",
            AguiEvent::StepFinished(_) => "STEP_FINISHED",
            AguiEvent::StateSnapshot(_) => "STATE_SNAPSHOT",
            AguiEvent::StateDelta(_) => "STATE_DELTA",
            AguiEvent::ActivitySnapshot(_) => "ACTIVITY_SNAPSHOT",
        })
        .collect()
}

#[tokio::test]
async fn single_signal_event_sequence() {
    let config = parse(r#"{"idle": {"on": {"START": "running"}}, "running": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let (stream, _ctx) = machine.stream(None, None, "idle", vec![Signal::new("START")], (), None);
    let events = collect(stream).await.unwrap();

    assert_eq!(type_names(&events), ["STEP_STARTED", "STATE_SNAPSHOT", "STATE_DELTA", "STEP_FINISHED"]);

    let AguiEvent::StepStarted(ev) = &events[0] else { panic!("expected StepStarted") };
    assert_eq!(ev.step_name, "START");

    let AguiEvent::StateSnapshot(ev) = &events[1] else { panic!("expected StateSnapshot") };
    assert_eq!(ev.snapshot, json!({"current_state": "idle"}));

    let AguiEvent::StateDelta(ev) = &events[2] else { panic!("expected StateDelta") };
    assert_eq!(ev.delta, json!([{"op": "replace", "path": "/current_state", "value": "running"}]));

    let AguiEvent::StepFinished(ev) = &events[3] else { panic!("expected StepFinished") };
    assert_eq!(ev.step_name, "START");
}

#[tokio::test]
async fn guard_and_action_emit_activity_snapshot() {
    let config = parse(r#"{"idle": {"on": {"START": {"target": "running", "guard": "ok", "actions": ["act"]}}}, "running": {}}"#);
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_guard("ok", guard_true);
    machine.register_action("act", no_op_action);

    let (stream, _ctx) = machine.stream(None, None, "idle", vec![Signal::new("START")], (), None);
    let events = collect(stream).await.unwrap();

    let activities: Vec<_> = events.iter().filter_map(|e| if let AguiEvent::ActivitySnapshot(ev) = e { Some(ev) } else { None }).collect();
    assert_eq!(activities.len(), 2);

    assert_eq!(activities[0].activity_type, "guard");
    assert_eq!(activities[0].content.kind, "guard");
    assert_eq!(activities[0].content.name, "ok");
    assert_eq!(activities[0].content.source, "on");
    assert_eq!(activities[0].content.result, Some(true));

    assert_eq!(activities[1].activity_type, "action");
    assert_eq!(activities[1].content.kind, "action");
    assert_eq!(activities[1].content.name, "act");
    assert_eq!(activities[1].content.source, "on");
    assert_eq!(activities[1].content.result, None);
}

#[tokio::test]
async fn guardless_transition_emits_no_guard_activity() {
    let config = parse(r#"{"idle": {"on": {"START": "running"}}, "running": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let (stream, _ctx) = machine.stream(None, None, "idle", vec![Signal::new("START")], (), None);
    let events = collect(stream).await.unwrap();

    assert!(events.iter().all(|e| !matches!(e, AguiEvent::ActivitySnapshot(_))));
}

#[tokio::test]
async fn always_cascade_emits_state_delta_and_guard_activity_per_hop() {
    let config = parse(
        r#"{
            "idle": {"always": [{"target": "mid", "guard": "always_true"}]},
            "mid": {"always": [{"target": "done", "guard": "always_true"}]},
            "done": {}
        }"#,
    );
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_guard("always_true", guard_true);

    let (stream, _ctx) = machine.stream(None, None, "idle", vec![], (), None);
    let events = collect(stream).await.unwrap();

    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| if let AguiEvent::StateDelta(ev) = e { Some(ev.delta[0]["value"].clone()) } else { None })
        .collect();
    assert_eq!(deltas, [json!("mid"), json!("done")]);

    let guard_results: Vec<_> = events
        .iter()
        .filter_map(|e| if let AguiEvent::ActivitySnapshot(ev) = e { Some(ev.content.result) } else { None })
        .collect();
    assert_eq!(guard_results, [Some(true), Some(true)]);
}

#[tokio::test]
async fn failed_guard_still_reports_activity() {
    let config = parse(
        r#"{
            "idle": {"on": {"START": [{"target": "blocked", "guard": "no"}, {"target": "running"}]}},
            "blocked": {},
            "running": {}
        }"#,
    );
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_guard("no", guard_false);

    let (stream, _ctx) = machine.stream(None, None, "idle", vec![Signal::new("START")], (), None);
    let events = collect(stream).await.unwrap();

    let activities: Vec<_> = events.iter().filter_map(|e| if let AguiEvent::ActivitySnapshot(ev) = e { Some(ev) } else { None }).collect();
    assert_eq!(activities.len(), 1);
    assert_eq!(activities[0].content.name, "no");
    assert_eq!(activities[0].content.result, Some(false));

    let AguiEvent::StateDelta(delta) = events.iter().find(|e| matches!(e, AguiEvent::StateDelta(_))).unwrap() else { unreachable!() };
    assert_eq!(delta.delta[0]["value"], json!("running"));
}

#[tokio::test]
async fn multi_hop_signal_gets_one_step_per_hop() {
    let config = parse(
        r#"{
            "idle": {"on": {"START": "mid"}},
            "mid": {"always": [{"target": "done", "guard": "always_true"}]},
            "done": {}
        }"#,
    );
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_guard("always_true", guard_true);

    let (stream, _ctx) = machine.stream(None, None, "idle", vec![Signal::new("START")], (), None);
    let events = collect(stream).await.unwrap();

    assert_eq!(
        type_names(&events),
        [
            "STEP_STARTED",
            "STATE_SNAPSHOT",
            "STATE_DELTA",
            "STEP_FINISHED",
            "STEP_STARTED",
            "STATE_SNAPSHOT",
            "ACTIVITY_SNAPSHOT",
            "STATE_DELTA",
            "STEP_FINISHED",
        ]
    );

    let AguiEvent::StepStarted(ev) = &events[0] else { panic!() };
    assert_eq!(ev.step_name, "START");
    let AguiEvent::StateSnapshot(ev) = &events[1] else { panic!() };
    assert_eq!(ev.snapshot, json!({"current_state": "idle"}));
    let AguiEvent::StateDelta(ev) = &events[2] else { panic!() };
    assert_eq!(ev.delta, json!([{"op": "replace", "path": "/current_state", "value": "mid"}]));

    let AguiEvent::StepStarted(ev) = &events[4] else { panic!() };
    assert_eq!(ev.step_name, "__always__");
    let AguiEvent::StateSnapshot(ev) = &events[5] else { panic!() };
    assert_eq!(ev.snapshot, json!({"current_state": "mid"}));
    let AguiEvent::StateDelta(ev) = &events[7] else { panic!() };
    assert_eq!(ev.delta, json!([{"op": "replace", "path": "/current_state", "value": "done"}]));
}

#[tokio::test]
async fn no_hop_on_all_guards_failing_still_opens_and_closes_step() {
    let config = parse(r#"{"idle": {"on": {"START": {"target": "running", "guard": "no"}}}, "running": {}}"#);
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_guard("no", guard_false);

    let (stream, _ctx) = machine.stream(None, None, "idle", vec![Signal::new("START")], (), None);
    let events = collect(stream).await.unwrap();

    assert_eq!(type_names(&events), ["STEP_STARTED", "STATE_SNAPSHOT", "ACTIVITY_SNAPSHOT", "STEP_FINISHED"]);
}

#[tokio::test]
async fn final_state_matches_equivalent_run() {
    let config = parse(r#"{"idle": {"on": {"START": "running"}}, "running": {"on": {"STOP": "idle"}}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();
    let signals = vec![Signal::new("START"), Signal::new("STOP")];

    let run_ctx = machine.run(None, None, "idle", signals.clone(), ()).await.unwrap();

    let (stream, _ctx) = machine.stream(None, None, "idle", signals, (), None);
    let events = collect(stream).await.unwrap();
    let last_delta = events.iter().rev().find_map(|e| if let AguiEvent::StateDelta(ev) = e { Some(ev) } else { None }).unwrap();
    assert_eq!(last_delta.delta[0]["value"], json!(run_ctx.current_state));
}

#[tokio::test]
async fn unhandled_action_error_propagates_from_stream() {
    let config = parse(r#"{"idle": {"on": {"START": {"target": "running", "actions": ["boom"]}}}, "running": {}}"#);
    let mut machine = StateMachine::<()>::from_config(config).unwrap();
    machine.register_action("boom", action_fails);

    let (stream, _ctx) = machine.stream(None, None, "idle", vec![Signal::new("START")], (), None);
    let err = collect(stream).await.unwrap_err();
    assert!(matches!(err, RunError::ActionFailed { .. }), "expected ActionFailed, got {err:?}");
}

#[tokio::test]
async fn state_accessor_drives_snapshot_and_delta() {
    let config = parse(r#"{"idle": {"on": {"START": {"target": "running", "actions": ["bump"]}}}, "running": {}}"#);
    let mut machine = StateMachine::<i32>::from_config(config).unwrap();
    machine.register_action("bump", bump_counter);

    let accessor: Arc<dyn Fn(&i32) -> serde_json::Value + Send + Sync> = Arc::new(|counter: &i32| json!({"counter": counter}));
    let (stream, _ctx) = machine.stream(None, None, "idle", vec![Signal::new("START")], 0, Some(accessor));
    let events = collect(stream).await.unwrap();

    let AguiEvent::StateSnapshot(snapshot) = events.iter().find(|e| matches!(e, AguiEvent::StateSnapshot(_))).unwrap() else { unreachable!() };
    assert_eq!(snapshot.snapshot, json!({"counter": 0}));

    let AguiEvent::StateDelta(delta) = events.iter().find(|e| matches!(e, AguiEvent::StateDelta(_))).unwrap() else { unreachable!() };
    assert_eq!(delta.delta, json!([{"op": "replace", "path": "/counter", "value": 1}]));
}

#[tokio::test]
async fn state_accessor_unchanged_output_emits_no_delta() {
    let config = parse(r#"{"idle": {"on": {"START": "running"}}, "running": {}}"#);
    let machine = StateMachine::<()>::from_config(config).unwrap();

    let accessor: Arc<dyn Fn(&()) -> serde_json::Value + Send + Sync> = Arc::new(|_: &()| json!({"fixed": "value"}));
    let (stream, _ctx) = machine.stream(None, None, "idle", vec![Signal::new("START")], (), Some(accessor));
    let events = collect(stream).await.unwrap();

    assert!(events.iter().all(|e| !matches!(e, AguiEvent::StateDelta(_))));
    assert_eq!(type_names(&events), ["STEP_STARTED", "STATE_SNAPSHOT", "STEP_FINISHED"]);
}

#[tokio::test]
async fn final_context_is_recoverable_after_draining_stream() {
    let config = parse(r#"{"idle": {"on": {"START": {"target": "running", "actions": ["bump"]}}}, "running": {}}"#);
    let mut machine = StateMachine::<i32>::from_config(config).unwrap();
    machine.register_action("bump", bump_counter);

    let (stream, ctx_rx) = machine.stream(None, None, "idle", vec![Signal::new("START")], 0, None);
    let _events = collect(stream).await.unwrap();
    let ctx = ctx_rx.await.expect("context should be sent once the stream is drained");
    assert_eq!(ctx.session, 1);
    assert_eq!(ctx.current_state, "running");
}
