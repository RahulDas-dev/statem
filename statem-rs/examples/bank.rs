//! Runnable example: a bank teller's transaction-posting bot -- the Rust counterpart to
//! `statem-py/examples/bank.py`. Run with `cargo run --example bank`.
//!
//! A teller walks a wire transfer through: identifying the transaction, resolving its required
//! fields (looping back to collect missing data when something's absent), and posting it to the
//! ledger -- including a real posting failure that gets corrected and retried.
//!
//! Demonstrates every hook the engine has, plus two Rust-specific things `bread.rs` didn't need:
//! - `on` guard chains: `txn_identify` tries two candidates in order (supported vs. unsupported).
//! - `always` auto-advance, including a chain of transitions firing within a single hop
//!   (`resolve_fields` -> `resolution_failed`, or `resolve_fields` -> `posting`).
//! - `error_state`: a real error raised inside an action (`call_ledger_api`) is caught by the
//!   engine and routed to `posting_failed` automatically.
//! - `entry` actions used as teller-facing prompts.
//! - A state (`collect_data`) re-entered from two different places in the graph.
//! - **Async guard/action as a manual trait impl**: `within_daily_limit`/`call_ledger_api` do
//!   real `.await`ing work, so (per the crate's MSRV note in `registry.rs`) they're a one-line
//!   `#[async_trait] impl Guard<..>`/`impl Action<..>` each, rather than a plain closure.
//! - **Threading `session` across several `run()` calls**: each teller turn is its own `run()`
//!   call (unlike `bread.rs`'s single batched call) because the *next* signal's data depends on
//!   what the teller says in response to *this* turn's bot output -- a real conversation, not a
//!   fixed script known upfront. Since `run()` moves `session` in and hands the whole `Context`
//!   back, threading it across turns means pulling `session` (and `current_state`) back out of
//!   each returned `Context` to feed into the next call.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::{json, Value};
use statem_rs::{Action, Context, Guard, Signal, StateConfig, StateMachine};

const SUPPORTED_TXN_TYPES: &[&str] = &["TRANSFER", "WITHDRAWAL", "DEPOSIT"];
const REQUIRED_FIELDS: &[&str] = &["txn_type", "from_account", "to_account", "amount"];
const DAILY_LIMIT: f64 = 1000.0;
const FROZEN_ACCOUNTS: &[&str] = &["ACC-2002"];

fn generate_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{prefix}-{nanos:x}{count:x}").to_uppercase()
}

#[derive(Debug, Default)]
struct BankSession {
    txn_id: Option<String>,
    txn_type: Option<String>,
    from_account: Option<String>,
    to_account: Option<String>,
    amount: Option<f64>,
    missing_fields: Vec<String>,
    last_error: Option<String>,
    receipt_id: Option<String>,
    log: Vec<String>,
}

type Ctx = Context<BankSession>;
type ActionResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn create_txn(ctx: &mut Ctx, _signal: &Signal) -> ActionResult {
    let txn_id = generate_id("TXN");
    ctx.session.log.push(format!("transaction {txn_id} opened"));
    ctx.session.txn_id = Some(txn_id);
    Ok(())
}

fn capture_identify_fields(ctx: &mut Ctx, signal: &Signal) -> ActionResult {
    let txn_type = signal.data.get("txn_type").and_then(Value::as_str).unwrap_or_default().to_string();
    let from_account = signal.data.get("from_account").and_then(Value::as_str).unwrap_or_default().to_string();
    ctx.session.log.push(format!("identified as {txn_type} from {from_account}"));
    ctx.session.txn_type = Some(txn_type);
    ctx.session.from_account = Some(from_account);
    Ok(())
}

fn log_unsupported_type(ctx: &mut Ctx, signal: &Signal) -> ActionResult {
    let txn_type = signal.data.get("txn_type").and_then(Value::as_str).unwrap_or("<missing>");
    ctx.session.log.push(format!("bot: sorry, {txn_type:?} is not a supported transaction type"));
    Ok(())
}

fn run_field_resolution(ctx: &mut Ctx, _signal: &Signal) -> ActionResult {
    let missing: Vec<String> = REQUIRED_FIELDS
        .iter()
        .filter(|&&name| match name {
            "txn_type" => ctx.session.txn_type.is_none(),
            "from_account" => ctx.session.from_account.is_none(),
            "to_account" => ctx.session.to_account.is_none(),
            "amount" => ctx.session.amount.is_none(),
            _ => false,
        })
        .map(|&name| name.to_string())
        .collect();
    ctx.session.missing_fields = missing;
    Ok(())
}

fn log_missing_fields(ctx: &mut Ctx, _signal: &Signal) -> ActionResult {
    ctx.session.log.push(format!("resolution incomplete, missing: {}", ctx.session.missing_fields.join(", ")));
    Ok(())
}

fn ask_teller_for_missing_fields(ctx: &mut Ctx, _signal: &Signal) -> ActionResult {
    let msg = if ctx.session.missing_fields.is_empty() {
        "bot: please provide a supported transaction type and try again".to_string()
    } else {
        format!("bot: please provide {}", ctx.session.missing_fields.join(", "))
    };
    ctx.session.log.push(msg);
    Ok(())
}

fn apply_teller_data(ctx: &mut Ctx, signal: &Signal) -> ActionResult {
    for (key, value) in &signal.data {
        match key.as_str() {
            "txn_type" => ctx.session.txn_type = value.as_str().map(String::from),
            "from_account" => ctx.session.from_account = value.as_str().map(String::from),
            "to_account" => ctx.session.to_account = value.as_str().map(String::from),
            "amount" => ctx.session.amount = value.as_f64(),
            _ => {}
        }
    }
    ctx.session.log.push(format!("teller provided: {:?}", signal.data));
    Ok(())
}

fn prompt_confirm_post(ctx: &mut Ctx, _signal: &Signal) -> ActionResult {
    let msg = format!(
        "bot: ready to post {} of {} from {} to {} -- confirm?",
        ctx.session.txn_type.as_deref().unwrap_or("?"),
        ctx.session.amount.unwrap_or_default(),
        ctx.session.from_account.as_deref().unwrap_or("?"),
        ctx.session.to_account.as_deref().unwrap_or("?"),
    );
    ctx.session.log.push(msg);
    Ok(())
}

fn reject_over_limit(ctx: &mut Ctx, _signal: &Signal) -> ActionResult {
    let msg = format!("amount {} exceeds daily limit {DAILY_LIMIT}", ctx.session.amount.unwrap_or_default());
    ctx.session.last_error = Some(msg.clone());
    ctx.session.log.push(format!("bot: rejected -- {msg}"));
    Ok(())
}

fn notify_failure(ctx: &mut Ctx, _signal: &Signal) -> ActionResult {
    let err = ctx.session.last_error.clone().unwrap_or_default();
    ctx.session.log.push(format!("bot: posting failed -- {err}"));
    Ok(())
}

fn print_receipt(ctx: &mut Ctx, _signal: &Signal) -> ActionResult {
    let msg = format!(
        "bot: posted! receipt {} -- {} {} {} -> {}",
        ctx.session.receipt_id.as_deref().unwrap_or("?"),
        ctx.session.txn_type.as_deref().unwrap_or("?"),
        ctx.session.amount.unwrap_or_default(),
        ctx.session.from_account.as_deref().unwrap_or("?"),
        ctx.session.to_account.as_deref().unwrap_or("?"),
    );
    ctx.session.log.push(msg);
    Ok(())
}

/// Real network hop, so it's an `async` action -- a manual trait impl rather than a plain
/// closure (see the module docs' MSRV note).
struct CallLedgerApi;

#[async_trait]
impl Action<BankSession> for CallLedgerApi {
    async fn execute(&self, ctx: &mut Ctx, _signal: &Signal) -> ActionResult {
        tokio::task::yield_now().await; // simulated network hop
        let to_account = ctx.session.to_account.clone().unwrap_or_default();
        if FROZEN_ACCOUNTS.contains(&to_account.as_str()) {
            let msg = format!("ledger rejected posting to {to_account} (account frozen)");
            ctx.session.last_error = Some(msg.clone());
            return Err(msg.into());
        }
        ctx.session.receipt_id = Some(generate_id("RCPT"));
        Ok(())
    }
}

fn txn_type_supported(_ctx: &Ctx, signal: &Signal) -> bool {
    signal.data.get("txn_type").and_then(Value::as_str).is_some_and(|t| SUPPORTED_TXN_TYPES.contains(&t))
}

fn txn_type_unsupported(ctx: &Ctx, signal: &Signal) -> bool {
    !txn_type_supported(ctx, signal)
}

fn all_fields_resolved(ctx: &Ctx, _signal: &Signal) -> bool {
    ctx.session.missing_fields.is_empty()
}

fn has_missing_fields(ctx: &Ctx, _signal: &Signal) -> bool {
    !ctx.session.missing_fields.is_empty()
}

fn exceeds_daily_limit(ctx: &Ctx, _signal: &Signal) -> bool {
    ctx.session.amount.is_some_and(|amt| amt > DAILY_LIMIT)
}

/// Real (simulated) fraud/limits-service lookup, so it's an `async` guard -- same reasoning as
/// [`CallLedgerApi`].
struct WithinDailyLimit;

#[async_trait]
impl Guard<BankSession> for WithinDailyLimit {
    async fn evaluate(&self, ctx: &Ctx, _signal: &Signal) -> bool {
        tokio::task::yield_now().await; // simulated fraud/limits-service lookup
        ctx.session.amount.map_or(true, |amt| amt <= DAILY_LIMIT)
    }
}

const CONFIG: &str = r#"{
    "idle": {
        "on": {"START_TXN": {"target": "txn_identify", "actions": ["create_txn"]}}
    },
    "txn_identify": {
        "on": {
            "IDENTIFY": [
                {"target": "resolve_fields", "guard": "txn_type_supported", "actions": ["capture_identify_fields"]},
                {"target": "resolution_failed", "guard": "txn_type_unsupported", "actions": ["log_unsupported_type"]}
            ]
        }
    },
    "resolve_fields": {
        "entry": ["run_field_resolution"],
        "always": [
            {"target": "posting", "guard": "all_fields_resolved"},
            {"target": "resolution_failed", "guard": "has_missing_fields", "actions": ["log_missing_fields"]}
        ]
    },
    "resolution_failed": {
        "entry": ["ask_teller_for_missing_fields"],
        "on": {"PROVIDE_DATA": {"target": "collect_data", "actions": ["apply_teller_data"]}}
    },
    "collect_data": {
        "always": [{"target": "resolve_fields"}]
    },
    "posting": {
        "entry": ["prompt_confirm_post"],
        "on": {
            "POST": [
                {"target": "posting_failed", "guard": "exceeds_daily_limit", "actions": ["reject_over_limit"]},
                {"target": "posting_pass", "guard": "within_daily_limit", "actions": ["call_ledger_api"]}
            ]
        },
        "error_state": "posting_failed"
    },
    "posting_failed": {
        "entry": ["notify_failure"],
        "on": {"CORRECT": {"target": "collect_data", "actions": ["apply_teller_data"]}}
    },
    "posting_pass": {
        "entry": ["print_receipt"]
    }
}"#;

fn data(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

// `current_thread`: the crate's dev-dependency on tokio only enables "macros"/"rt" (matching
// what `#[tokio::test]` needs elsewhere in this crate), not "rt-multi-thread".
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let config: IndexMap<String, StateConfig> = serde_json::from_str(CONFIG).expect("config should deserialize");
    let mut machine = StateMachine::<BankSession>::from_config(config).expect("transition targets should be valid");

    machine.register_action("create_txn", create_txn);
    machine.register_action("capture_identify_fields", capture_identify_fields);
    machine.register_action("log_unsupported_type", log_unsupported_type);
    machine.register_action("run_field_resolution", run_field_resolution);
    machine.register_action("log_missing_fields", log_missing_fields);
    machine.register_action("ask_teller_for_missing_fields", ask_teller_for_missing_fields);
    machine.register_action("apply_teller_data", apply_teller_data);
    machine.register_action("prompt_confirm_post", prompt_confirm_post);
    machine.register_action("reject_over_limit", reject_over_limit);
    machine.register_action("call_ledger_api", CallLedgerApi);
    machine.register_action("notify_failure", notify_failure);
    machine.register_action("print_receipt", print_receipt);

    machine.register_guard("txn_type_supported", txn_type_supported);
    machine.register_guard("txn_type_unsupported", txn_type_unsupported);
    machine.register_guard("all_fields_resolved", all_fields_resolved);
    machine.register_guard("has_missing_fields", has_missing_fields);
    machine.register_guard("exceeds_daily_limit", exceeds_daily_limit);
    machine.register_guard("within_daily_limit", WithinDailyLimit);

    machine.validate_registries().expect("every guard/action referenced in CONFIG should be registered above");

    // (event, data, what the teller says this turn)
    let conversation: [(&str, HashMap<String, Value>, &str); 6] = [
        ("START_TXN", data(&[]), "I'd like to start a new transaction."),
        ("IDENTIFY", data(&[("txn_type", json!("TRANSFER")), ("from_account", json!("ACC-1001"))]), "It's a transfer from ACC-1001."),
        ("PROVIDE_DATA", data(&[("amount", json!(500.0)), ("to_account", json!("ACC-2002"))]), "Send $500 to ACC-2002."),
        ("POST", data(&[]), "Go ahead and post it."),
        ("CORRECT", data(&[("to_account", json!("ACC-3003"))]), "Oh -- use ACC-3003 instead."),
        ("POST", data(&[]), "Post it now."),
    ];

    let mut state = "idle".to_string();
    let mut session = BankSession::default();
    let mut log_cursor = 0usize;

    for (event, event_data, teller_says) in conversation {
        println!("\nTeller: {teller_says}");
        let signal = Signal::with_data(event, event_data);
        let ctx = machine.run(Some("bank-demo-001".to_string()), state, vec![signal], session).await.expect("run should succeed");

        for line in &ctx.session.log[log_cursor..] {
            println!("  {line}");
        }
        log_cursor = ctx.session.log.len();
        state = ctx.current_state.clone();
        println!("  -> state: {state}");
        session = ctx.session;
    }

    println!("\nFinal state: {state}");
    println!("Receipt: {:?}", session.receipt_id);
}
