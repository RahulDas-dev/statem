# Examples

Three runnable, fully-tested example scripts live in
[`examples/`](https://github.com/RahulDas-dev/statem/tree/main/examples) in the repository. All
are shown in full below, kept in sync automatically with the actual source files.

## Bakery process (`examples/bread.py`)

A small bakery process (`idle → mixing → baking → cooling → done`) that exercises guards,
actions, an `error_state` fallback, and an `always`-transition that auto-advances
`cooling → done` once the oven has cooled — all triggered by a single `TIMER_DONE` signal.

Its graph, rendered with {py:func}`to_mermaid <statem.to_mermaid>`:

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

```{literalinclude} ../examples/bread.py
:language: python
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

```{literalinclude} ../examples/bank.py
:language: python
```

## Pizza order bot (`examples/pizza.py`, streaming)

The largest example: an order-to-delivery pizza bot (payment, quality-check retries, delivery
retries, `CANCEL` escape hatches at multiple points) that also demonstrates
[`stream()`](streaming.md) — `run_streaming_demo()` drives one signal through the machine and
prints the raw AG-UI event sequence (`STEP_STARTED`, `STATE_SNAPSHOT`, `ACTIVITY_SNAPSHOT`,
`STATE_DELTA`, `STEP_FINISHED`) as it happens, alongside two plain `run()` demos for comparison.
Requires the `agui` extra (`pip install statem[agui]`) for the streaming portion only.

```mermaid
stateDiagram-v2
    [*] --> order_received
    order_received --> payment_processing: CONFIRM
    payment_processing --> preparing: always [payment_approved]
    payment_processing --> payment_failed: always [payment_declined]
    payment_processing --> payment_failed: error
    payment_failed --> payment_processing: RETRY_PAYMENT
    payment_failed --> cancelled: CANCEL
    preparing --> baking: always [prep_complete]
    baking --> quality_check: BAKING_DONE
    quality_check --> ready: always [quality_passed]
    quality_check --> remaking: always [quality_failed]
    remaking --> preparing: always
    ready --> assigning_driver: DISPATCH
    assigning_driver --> out_for_delivery: always [driver_found]
    out_for_delivery --> delivered: DELIVERED
    out_for_delivery --> delivery_failed: FAILED
    out_for_delivery --> delivery_failed: error
    delivery_failed --> assigning_driver: RETRY_DELIVERY
    delivery_failed --> cancelled: CANCEL
```

```bash
uv run python examples/pizza.py
```

```{literalinclude} ../examples/pizza.py
:language: python
```
