//! Covers `Context::trace()` / `Trace`'s `Display` impl.

use statem_rs::{Context, HookSource, ResultEntry, ResultKind};

fn ctx_with(history: Vec<&str>, results: Vec<ResultEntry>) -> Context<()> {
    let mut ctx = Context::new(history[0], (), None, None);
    ctx.history = history.into_iter().map(String::from).collect();
    ctx.results = results;
    ctx
}

fn entry(state: &str, source: HookSource, name: &str, kind: ResultKind) -> ResultEntry {
    ResultEntry { state: state.into(), source, name: name.into(), kind }
}

#[test]
fn zero_hops_singular_final_state_only() {
    let ctx = ctx_with(vec!["idle"], vec![]);
    let text = ctx.trace().to_string();

    assert!(text.starts_with("Transitions (0 hops):"));
    assert!(text.contains("Final state: idle"));
    assert!(!text.contains("hop 1:"));
}

#[test]
fn one_hop_uses_singular_hop_not_hops() {
    let ctx = ctx_with(vec!["idle", "running"], vec![]);
    let text = ctx.trace().to_string();

    assert!(text.contains("Transitions (1 hop):"));
    assert!(!text.contains("1 hops"));
    assert!(text.contains("hop 1: idle -> running"));
    assert!(text.contains("Final state: running"));
}

#[test]
fn multi_hop_numbers_each_hop_and_pluralizes() {
    let ctx = ctx_with(vec!["a", "b", "c"], vec![]);
    let text = ctx.trace().to_string();

    assert!(text.contains("Transitions (2 hops):"));
    assert!(text.contains("hop 1: a -> b"));
    assert!(text.contains("hop 2: b -> c"));
    assert!(text.contains("Final state: c"));
}

#[test]
fn guard_row_shows_value_action_row_does_not() {
    let ctx = ctx_with(
        vec!["idle", "running"],
        vec![
            entry("idle", HookSource::On, "can_start", ResultKind::Guard(true)),
            entry("idle", HookSource::On, "create_txn", ResultKind::Action),
        ],
    );
    let text = ctx.trace().to_string();

    let guard_line = text.lines().find(|line| line.contains("can_start")).unwrap();
    let action_line = text.lines().find(|line| line.contains("create_txn")).unwrap();

    assert!(guard_line.contains("guard"));
    assert!(guard_line.trim_end().ends_with("= true"));
    assert!(action_line.contains("action"));
    assert!(!action_line.contains('='));
}

#[test]
fn consecutive_same_source_rows_dedup_the_label() {
    let ctx = ctx_with(
        vec!["idle"],
        vec![
            entry("idle", HookSource::On, "g1", ResultKind::Guard(true)),
            entry("idle", HookSource::On, "a1", ResultKind::Action),
            entry("idle", HookSource::Entry, "a2", ResultKind::Action),
        ],
    );
    let text = ctx.trace().to_string();

    let g1_line = text.lines().find(|line| line.contains("g1")).unwrap();
    let a1_line = text.lines().find(|line| line.contains("a1")).unwrap();
    let a2_line = text.lines().find(|line| line.contains("a2")).unwrap();

    assert!(g1_line.trim_start().starts_with("on"));
    assert!(a1_line.trim_start().starts_with("action"), "expected deduped (blank) label, got: {a1_line:?}");
    assert!(a2_line.trim_start().starts_with("entry"), "source changed, label should reappear: {a2_line:?}");
}

#[test]
fn state_with_no_results_renders_empty_section() {
    let ctx = ctx_with(vec!["a", "b"], vec![entry("a", HookSource::On, "act", ResultKind::Action)]);
    let text = ctx.trace().to_string();

    // "b" (the final state) has no ResultEntry -- shouldn't panic, shouldn't emit a stray row.
    assert!(text.contains("Final state: b"));
    assert!(!text.contains("act\n\n"));
}

#[test]
fn display_composes_with_write_and_to_string() {
    use std::fmt::Write;

    let ctx = ctx_with(vec!["idle"], vec![]);
    let mut buf = String::new();
    write!(buf, "{}", ctx.trace()).unwrap();

    assert_eq!(buf, ctx.trace().to_string());
}
