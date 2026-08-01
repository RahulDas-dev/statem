# ruff: noqa: ARG001
"""Runnable example: a bank teller's transaction-posting bot modeled as a StateMachine.

A teller walks a wire transfer through: identifying the transaction, resolving its required
fields (looping back to collect missing data from the teller when something's absent), and
posting it to the ledger -- including a real posting failure that gets corrected and retried.

Demonstrates every hook the engine has:
- `on` guard chains: `txn_identify` tries two candidates in order (supported vs. unsupported type).
- `always` auto-advance, including a *chain* of several always-transitions firing within a single
  turn (`resolve_fields` -> `resolution_failed`, or `resolve_fields` -> `posting`).
- `error_state`: a real `RuntimeError` raised deep inside an async action (`call_ledger_api`) is
  caught by the engine and routed to `posting_failed` automatically.
- `entry` actions used as teller-facing prompts (`prompt_confirm_post`, `ask_teller_for_missing_fields`).
- Both sync and async guards/actions, and a state (`collect_data`) re-entered from two different
  places in the graph (`resolution_failed` and `posting_failed`), showing this is a real graph,
  not a linear pipeline.
"""

from __future__ import annotations

import asyncio
import uuid
from dataclasses import dataclass, field

from statem import Context, Signal, StateMachine

SUPPORTED_TXN_TYPES = {"TRANSFER", "WITHDRAWAL", "DEPOSIT"}
REQUIRED_FIELDS = ("txn_type", "from_account", "to_account", "amount")
DAILY_LIMIT = 1000.0
FROZEN_ACCOUNTS = {"ACC-2002"}


@dataclass
class BankSession:
    txn_id: str | None = None
    txn_type: str | None = None
    from_account: str | None = None
    to_account: str | None = None
    amount: float | None = None
    missing_fields: list[str] = field(default_factory=list)
    last_error: str | None = None
    receipt_id: str | None = None
    log: list[str] = field(default_factory=list)


def create_txn(ctx: Context[BankSession], signal: Signal) -> None:
    ctx.session.txn_id = f"TXN-{uuid.uuid4().hex[:8].upper()}"
    ctx.session.log.append(f"transaction {ctx.session.txn_id} opened")


def capture_identify_fields(ctx: Context[BankSession], signal: Signal) -> None:
    ctx.session.txn_type = signal.data["txn_type"]
    ctx.session.from_account = signal.data["from_account"]
    ctx.session.log.append(f"identified as {ctx.session.txn_type} from {ctx.session.from_account}")


def log_unsupported_type(ctx: Context[BankSession], signal: Signal) -> None:
    ctx.session.log.append(f"bot: sorry, {signal.data.get('txn_type')!r} is not a supported transaction type")


def run_field_resolution(ctx: Context[BankSession], signal: Signal) -> None:
    session = ctx.session
    session.missing_fields = [name for name in REQUIRED_FIELDS if getattr(session, name) is None]


def log_missing_fields(ctx: Context[BankSession], signal: Signal) -> None:
    ctx.session.log.append(f"resolution incomplete, missing: {', '.join(ctx.session.missing_fields)}")


def ask_teller_for_missing_fields(ctx: Context[BankSession], signal: Signal) -> None:
    if ctx.session.missing_fields:
        ctx.session.log.append(f"bot: please provide {', '.join(ctx.session.missing_fields)}")
    else:
        ctx.session.log.append("bot: please provide a supported transaction type and try again")


def apply_teller_data(ctx: Context[BankSession], signal: Signal) -> None:
    for key, value in signal.data.items():
        setattr(ctx.session, key, value)
    ctx.session.log.append(f"teller provided: {signal.data}")


def prompt_confirm_post(ctx: Context[BankSession], signal: Signal) -> None:
    session = ctx.session
    ctx.session.log.append(
        f"bot: ready to post {session.txn_type} of {session.amount} from "
        f"{session.from_account} to {session.to_account} -- confirm?"
    )


def reject_over_limit(ctx: Context[BankSession], signal: Signal) -> None:
    ctx.session.last_error = f"amount {ctx.session.amount} exceeds daily limit {DAILY_LIMIT}"
    ctx.session.log.append(f"bot: rejected -- {ctx.session.last_error}")


async def call_ledger_api(ctx: Context[BankSession], signal: Signal) -> None:
    await asyncio.sleep(0)  # simulated network hop
    if ctx.session.to_account in FROZEN_ACCOUNTS:
        ctx.session.last_error = f"ledger rejected posting to {ctx.session.to_account} (account frozen)"
        raise RuntimeError(ctx.session.last_error)
    ctx.session.receipt_id = f"RCPT-{uuid.uuid4().hex[:8].upper()}"


def notify_failure(ctx: Context[BankSession], signal: Signal) -> None:
    ctx.session.log.append(f"bot: posting failed -- {ctx.session.last_error}")


def print_receipt(ctx: Context[BankSession], signal: Signal) -> None:
    session = ctx.session
    ctx.session.log.append(
        f"bot: posted! receipt {session.receipt_id} -- "
        f"{session.txn_type} {session.amount} {session.from_account} -> {session.to_account}"
    )


def txn_type_supported(ctx: Context[BankSession], signal: Signal) -> bool:
    return signal.data.get("txn_type") in SUPPORTED_TXN_TYPES


def txn_type_unsupported(ctx: Context[BankSession], signal: Signal) -> bool:
    return not txn_type_supported(ctx, signal)


def all_fields_resolved(ctx: Context[BankSession], signal: Signal) -> bool:
    return not ctx.session.missing_fields


def has_missing_fields(ctx: Context[BankSession], signal: Signal) -> bool:
    return bool(ctx.session.missing_fields)


def exceeds_daily_limit(ctx: Context[BankSession], signal: Signal) -> bool:
    return ctx.session.amount is not None and ctx.session.amount > DAILY_LIMIT


async def within_daily_limit(ctx: Context[BankSession], signal: Signal) -> bool:
    await asyncio.sleep(0)  # simulated fraud/limits-service lookup
    return not exceeds_daily_limit(ctx, signal)


CONFIG = {
    "idle": {
        "on": {"START_TXN": {"target": "txn_identify", "actions": ["create_txn"]}},
    },
    "txn_identify": {
        "on": {
            "IDENTIFY": [
                {"target": "resolve_fields", "guard": "txn_type_supported", "actions": ["capture_identify_fields"]},
                {"target": "resolution_failed", "guard": "txn_type_unsupported", "actions": ["log_unsupported_type"]},
            ]
        },
    },
    "resolve_fields": {
        "entry": ["run_field_resolution"],
        "always": [
            {"target": "posting", "guard": "all_fields_resolved"},
            {"target": "resolution_failed", "guard": "has_missing_fields", "actions": ["log_missing_fields"]},
        ],
    },
    "resolution_failed": {
        "entry": ["ask_teller_for_missing_fields"],
        "on": {"PROVIDE_DATA": {"target": "collect_data", "actions": ["apply_teller_data"]}},
    },
    "collect_data": {
        "always": [{"target": "resolve_fields"}],
    },
    "posting": {
        "entry": ["prompt_confirm_post"],
        "on": {
            "POST": [
                {"target": "posting_failed", "guard": "exceeds_daily_limit", "actions": ["reject_over_limit"]},
                {"target": "posting_pass", "guard": "within_daily_limit", "actions": ["call_ledger_api"]},
            ]
        },
        "error_state": "posting_failed",
    },
    "posting_failed": {
        "entry": ["notify_failure"],
        "on": {"CORRECT": {"target": "collect_data", "actions": ["apply_teller_data"]}},
    },
    "posting_pass": {
        "entry": ["print_receipt"],
    },
}

ACTIONS = {
    "create_txn": create_txn,
    "capture_identify_fields": capture_identify_fields,
    "log_unsupported_type": log_unsupported_type,
    "run_field_resolution": run_field_resolution,
    "log_missing_fields": log_missing_fields,
    "ask_teller_for_missing_fields": ask_teller_for_missing_fields,
    "apply_teller_data": apply_teller_data,
    "prompt_confirm_post": prompt_confirm_post,
    "reject_over_limit": reject_over_limit,
    "call_ledger_api": call_ledger_api,
    "notify_failure": notify_failure,
    "print_receipt": print_receipt,
}

GUARDS = {
    "txn_type_supported": txn_type_supported,
    "txn_type_unsupported": txn_type_unsupported,
    "all_fields_resolved": all_fields_resolved,
    "has_missing_fields": has_missing_fields,
    "exceeds_daily_limit": exceeds_daily_limit,
    "within_daily_limit": within_daily_limit,
}

# (event, data, what the teller says this turn)
CONVERSATION = [
    ("START_TXN", {}, "I'd like to start a new transaction."),
    ("IDENTIFY", {"txn_type": "TRANSFER", "from_account": "ACC-1001"}, "It's a transfer from ACC-1001."),
    ("PROVIDE_DATA", {"amount": 500.0, "to_account": "ACC-2002"}, "Send $500 to ACC-2002."),
    ("POST", {}, "Go ahead and post it."),
    ("CORRECT", {"to_account": "ACC-3003"}, "Oh -- use ACC-3003 instead."),
    ("POST", {}, "Post it now."),
]


async def main() -> None:
    machine = StateMachine.from_dict(CONFIG, action_dict=ACTIONS, guard_dict=GUARDS)
    session = BankSession()
    state = "idle"
    cursor = 0

    for event, data, teller_says in CONVERSATION:
        print(f"\nTeller: {teller_says}")
        state = await machine.run(
            run_id="bank-demo-001",
            state_name=state,
            events=Signal(event=event, data=data),
            session=session,
        )
        for line in session.log[cursor:]:
            print(f"  {line}")
        cursor = len(session.log)
        print(f"  -> state: {state}")

    print(f"\nFinal state: {state}")
    print(f"Receipt: {session.receipt_id}")


if __name__ == "__main__":
    asyncio.run(main())
