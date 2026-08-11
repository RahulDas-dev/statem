//! Runnable example: a bakery process modeled as a `StateMachine` -- the Rust counterpart to
//! `statem-py/examples/bread.py`. Run with `cargo run --example bread`.
//!
//! Demonstrates:
//! - a config parsed from JSON (`on` / `always` / `entry` / `error_state`)
//! - guards/actions registered by name as plain closures
//! - an arbitrary `session` type (here, a plain struct) -- unlike Python, where a mutated
//!   `session` is visible to the caller because it was passed by reference, Rust's ownership
//!   model means `session` comes back out via the `Context<T>` that `run()` returns (see
//!   `StateMachine::run`'s docs)
//! - the `always`-transition auto-advance mechanism
//! - `Context::trace()` for a hop-by-hop report of every guard/action that fired
//!
//! Rather than looping over one signal at a time the way the Python example does (there, mostly
//! so it can print progress per turn), this passes every signal to a single `run()` call --
//! `run()` already accepts a `Vec<Signal>`, and one call gives back one `Context` whose
//! `history`/`results` cover the whole run, which is a nicer fit for `trace()`.

use indexmap::IndexMap;
use statem_rs::{Context, Signal, StateConfig, StateMachine};

#[derive(Debug, Default)]
struct BakingSession {
    oven_temp_c: i32,
    ingredients_checked: bool,
    log: Vec<String>,
}

const CONFIG: &str = r#"{
    "idle": {
        "on": {"START": {"target": "mixing", "actions": ["check_ingredients"]}}
    },
    "mixing": {
        "on": {"MIXED": {"target": "baking", "guard": "ingredients_ready", "actions": ["preheat_oven"]}},
        "error_state": "failed"
    },
    "baking": {
        "on": {"TIMER_DONE": {"target": "cooling", "actions": ["start_cooling"]}}
    },
    "cooling": {
        "always": [{"target": "done", "guard": "oven_is_cool"}]
    },
    "done": {
        "entry": ["plate_cake"]
    },
    "failed": {}
}"#;

// `current_thread`: the crate's dev-dependency on tokio only enables "macros"/"rt" (matching
// what `#[tokio::test]` needs elsewhere in this crate), not "rt-multi-thread".
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let config: IndexMap<String, StateConfig> = serde_json::from_str(CONFIG).expect("config should deserialize");
    let mut machine = StateMachine::<BakingSession>::from_config(config).expect("transition targets should be valid");

    machine.register_action("check_ingredients", |ctx: &mut Context<BakingSession>, _signal: &Signal| {
        ctx.session.ingredients_checked = true;
        ctx.session.log.push("ingredients checked".to_string());
        Ok(())
    });
    machine.register_action("preheat_oven", |ctx: &mut Context<BakingSession>, _signal: &Signal| {
        ctx.session.oven_temp_c = 180;
        ctx.session.log.push("oven preheated to 180C".to_string());
        Ok(())
    });
    machine.register_action("start_cooling", |ctx: &mut Context<BakingSession>, _signal: &Signal| {
        ctx.session.oven_temp_c = 25;
        ctx.session.log.push("cake pulled, cooling started".to_string());
        Ok(())
    });
    machine.register_action("plate_cake", |ctx: &mut Context<BakingSession>, _signal: &Signal| {
        ctx.session.log.push("cake plated".to_string());
        Ok(())
    });

    machine.register_guard("ingredients_ready", |ctx: &Context<BakingSession>, _signal: &Signal| ctx.session.ingredients_checked);
    machine.register_guard("oven_is_cool", |ctx: &Context<BakingSession>, _signal: &Signal| ctx.session.oven_temp_c <= 30);

    machine.validate_registries().expect("every guard/action referenced in CONFIG should be registered above");

    let events = vec![Signal::new("START"), Signal::new("MIXED"), Signal::new("TIMER_DONE")];
    let ctx = machine
        .run(Some("bake-001".to_string()), None, "idle", events, BakingSession::default())
        .await
        .expect("run should succeed");

    println!("final state: {}", ctx.current_state);
    println!("session log:");
    for line in &ctx.session.log {
        println!("  - {line}");
    }
    println!();
    println!("{}", ctx.trace());
}
