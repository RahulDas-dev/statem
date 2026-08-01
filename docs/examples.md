# Examples

Two runnable, fully-tested example scripts live in
[`examples/`](https://github.com/RahulDas-dev/statem/tree/main/examples) in the repository. Both
are shown in full below, kept in sync automatically with the actual source files.

## Bakery process (`examples/bread.py`)

A small bakery process (`idle → mixing → baking → cooling → done`) that exercises guards,
actions, an `error_state` fallback, and an `always`-transition that auto-advances
`cooling → done` once the oven has cooled — all triggered by a single `TIMER_DONE` signal.

Its graph, rendered with [`to_mermaid`](api.md#statem.to_mermaid):

```mermaid
stateDiagram-v2
    [*] --> idle
    idle --> mixing: START
    mixing --> baking: MIXED [ingredients_ready]
    mixing --> failed: error
    baking --> cooling: TIMER_DONE
    cooling --> done: always [oven_is_cool]
```

```bash
uv run python examples/bread.py
```

```python
--8 < --"examples/bread.py"
```

## Bank teller bot (`examples/bank.py`)

A richer example: a bank teller's transaction-posting bot that exercises every hook in one
conversation — `on` guard chains (a two-candidate check for supported vs. unsupported
transaction types), a multi-hop `always` cascade (missing fields loop back to ask the teller,
then re-resolve), and `error_state` recovering from a real exception raised inside an async
action (a simulated ledger call), followed by a correction and retry.

Its graph shows both failure mechanisms side by side -- `POST`'s guard chain rejecting
over-the-limit amounts up front, and the separate `error`-labeled edge from `error_state`
catching the ledger call's exception:

```mermaid
stateDiagram-v2
    [*] --> idle
    idle --> txn_identify: START_TXN
    txn_identify --> resolve_fields: IDENTIFY [txn_type_supported]
    txn_identify --> resolution_failed: IDENTIFY [txn_type_unsupported]
    resolve_fields --> posting: always [all_fields_resolved]
    resolve_fields --> resolution_failed: always [has_missing_fields]
    resolution_failed --> collect_data: PROVIDE_DATA
    collect_data --> resolve_fields: always
    posting --> posting_failed: POST [exceeds_daily_limit]
    posting --> posting_pass: POST [within_daily_limit]
    posting --> posting_failed: error
    posting_failed --> collect_data: CORRECT
```

```bash
uv run python examples/bank.py
```

```python
--8 < --"examples/bank.py"
```
