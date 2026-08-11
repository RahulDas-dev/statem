# ruff: noqa: ARG001
"""Pizza Order Tracker - a Domino's-style order lifecycle modelled as a StateMachine

Demonstrates every engine feature on a problem everyone instantly recognises:

State graph (12 states)
-----------------------

stateDiagram-v2
    [*] --> order_received
    order_received --> payment_processing: CONFIRM

    payment_processing --> preparing: payment_approved
    payment_processing --> payment_failed: payment_declined
    payment_processing --> payment_failed: error

    payment_failed --> payment_processing: RETRY_PAYMENT
    payment_failed --> cancelled: CANCEL

    preparing --> baking: prep_complete
    baking --> quality_check: BAKING_DONE

    quality_check --> ready: quality_passed
    quality_check --> remaking: quality_failed
    remaking --> preparing: retry

    ready --> assigning_driver: DISPATCH
    assigning_driver --> out_for_delivery: driver_found

    out_for_delivery --> delivered: DELIVERED
    out_for_delivery --> delivery_failed: FAILED
    out_for_delivery --> delivery_failed: error

    delivery_failed --> assigning_driver: RETRY_DELIVERY
    delivery_failed --> cancelled: CANCEL

    delivered --> [*]
    cancelled --> [*]
Interesting paths in the demo
-----------------------------

Run 1 (happy path with quality-fail loop):
  CONFIRM -> [payment ok] -> preparing -> baking
  BAKING_DONE -> quality_check [score 5 -> fail] -> remaking -> preparing -> baking
  BAKING_DONE -> quality_check [score 9 -> pass] -> ready
  DISPATCH -> [driver found] -> out_for_delivery
  DELIVERED -> delivered ✓

Run 2 (payment declined -> retry -> success):
  CONFIRM(cash) -> [payment declined] -> payment_failed
  RETRY_PAYMENT(card) -> [payment ok] -> preparing -> ... -> delivered ✓
"""

from __future__ import annotations

import asyncio
import sys
import uuid
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from statem import Context, Signal, StateMachine
from statem.diagram import to_mermaid

if TYPE_CHECKING:
    from ag_ui.core import BaseEvent

if sys.stdout.encoding.lower() != "utf-8":  # notes below use emoji; avoid UnicodeEncodeError on cp1252 consoles
    sys.stdout.reconfigure(encoding="utf-8")

QUALITY_PASS_THRESHOLD = 7


# --- Session ---
@dataclass
class PizzaSession:
    order_id: str | None = None
    customer_name: str = ""
    pizza: str = ""
    address: str = ""
    payment_method: str = ""  # "card" | "cash"
    payment_result: str | None = None  # "approved" | "declined"
    prep_status: str | None = None  # "done" | None
    quality_score: int = 0  # 0-10; ≥7 passes
    quality_attempts: int = 0
    driver_id: str | None = None
    delivery_result: str | None = None
    notes: list[str] = field(default_factory=list)


# --- Actions ---
def create_order(ctx: Context[PizzaSession], sig: Signal) -> str:
    ctx.session.order_id = f"ORD-{uuid.uuid4().hex[:6].upper()}"
    for k, v in sig.data.items():
        setattr(ctx.session, k, v)
    ctx.session.notes.append(f"order {ctx.session.order_id} created for {ctx.session.customer_name}")
    return ctx.session.order_id


def notify_received(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.notes.append(
        f"SMS -> {ctx.session.customer_name}: we got your {ctx.session.pizza} order! "
        f"Delivering to {ctx.session.address}"
    )


async def charge_card(ctx: Context[PizzaSession], sig: Signal) -> str:
    await asyncio.sleep(0)  # simulated gateway round-trip
    if ctx.session.payment_method == "cash":
        ctx.session.payment_result = "declined"
        return "declined"
    ctx.session.payment_result = "approved"
    ctx.session.notes.append(f"gateway: {ctx.session.payment_method} charged ✓")
    return "approved"


def notify_payment_success(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.notes.append("SMS - payment confirmed, kitchen notified")


def notify_payment_failure(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.notes.append("SMS - payment failed, please retry or cancel")


def reset_payment(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.payment_result = None
    new_method = sig.data.get("payment_method", ctx.session.payment_method)
    ctx.session.payment_method = new_method
    ctx.session.notes.append(f"payment method updated to {new_method}")


def start_preparation(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.prep_status = "done"  # kitchen is fast in this restaurant
    ctx.session.notes.append(f"kitchen: started {ctx.session.pizza} (attempt {ctx.session.quality_attempts + 1})")


def log_prep_complete(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.notes.append("kitchen: prep done, going into oven")


def start_baking(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.notes.append("oven: baking started 🔥")


def run_quality_check(ctx: Context[PizzaSession], sig: Signal) -> int:
    ctx.session.quality_attempts += 1
    # First attempt: underbaked (score 5). Subsequent: perfect (score 9).
    ctx.session.quality_score = 9 if ctx.session.quality_attempts > 1 else 5
    ctx.session.notes.append(
        f"QC attempt {ctx.session.quality_attempts}: score {ctx.session.quality_score}/10 - "
        f"{'pass' if ctx.session.quality_score >= QUALITY_PASS_THRESHOLD else 'fail - remaking'}"
    )
    return ctx.session.quality_score


def log_quality_fail(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.notes.append("QC: sending back to kitchen for remake")


def reset_for_remake(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.prep_status = None  # will be set again by start_preparation
    ctx.session.quality_score = 0


def notify_ready(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.notes.append(f"SMS - {ctx.session.customer_name}: your {ctx.session.pizza} is ready! 🍕")


async def find_driver(ctx: Context[PizzaSession], sig: Signal) -> str:
    await asyncio.sleep(0)  # simulated dispatch API
    ctx.session.driver_id = f"DRV-{uuid.uuid4().hex[:4].upper()}"
    ctx.session.notes.append(f"dispatch: driver {ctx.session.driver_id} assigned")
    return ctx.session.driver_id


def notify_dispatch(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.notes.append(f"SMS - {ctx.session.customer_name}: {ctx.session.driver_id} is on the way! 🛵")


def complete_delivery(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.delivery_result = "delivered"
    ctx.session.notes.append(f"✓ delivered to {ctx.session.address} - enjoy your {ctx.session.pizza}!")


def handle_delivery_failure(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.delivery_result = "failed"
    ctx.session.driver_id = None
    ctx.session.notes.append(f"✗ delivery failed at {ctx.session.address} - notifying customer")


def reset_driver(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.driver_id = None
    ctx.session.delivery_result = None
    ctx.session.notes.append("retrying driver assignment")


def notify_cancellation(ctx: Context[PizzaSession], sig: Signal) -> None:
    ctx.session.notes.append(f"SMS - {ctx.session.customer_name}: order cancelled, refund issued")


# --- Guards ---


def payment_approved(ctx: Context[PizzaSession], sig: Signal) -> bool:
    return ctx.session.payment_result == "approved"


def payment_declined(ctx: Context[PizzaSession], sig: Signal) -> bool:
    return ctx.session.payment_result == "declined"


def prep_complete(ctx: Context[PizzaSession], sig: Signal) -> bool:
    return ctx.session.prep_status == "done"


def quality_passed(ctx: Context[PizzaSession], sig: Signal) -> bool:
    return ctx.session.quality_score >= QUALITY_PASS_THRESHOLD


def quality_failed(ctx: Context[PizzaSession], sig: Signal) -> bool:
    return 0 < ctx.session.quality_score < QUALITY_PASS_THRESHOLD


def driver_found(ctx: Context[PizzaSession], sig: Signal) -> bool:
    return ctx.session.driver_id is not None


# --- Config ---

CONFIG: dict = {
    "order_received": {
        "on": {
            "CONFIRM": {
                "target": "payment_processing",
                "actions": ["create_order", "notify_received"],
            }
        }
    },
    "payment_processing": {
        "entry": ["charge_card"],
        "always": [
            {"target": "preparing", "guard": "payment_approved", "actions": ["notify_payment_success"]},
            {"target": "payment_failed", "guard": "payment_declined", "actions": ["notify_payment_failure"]},
        ],
        "error_state": "payment_failed",
    },
    "payment_failed": {
        "on": {
            "RETRY_PAYMENT": {"target": "payment_processing", "actions": ["reset_payment"]},
            "CANCEL": {"target": "cancelled", "actions": ["notify_cancellation"]},
        }
    },
    "preparing": {
        "entry": ["start_preparation"],
        "always": [
            {"target": "baking", "guard": "prep_complete", "actions": ["log_prep_complete"]},
        ],
    },
    "baking": {
        "entry": ["start_baking"],
        "on": {"BAKING_DONE": {"target": "quality_check"}},
    },
    "quality_check": {
        "entry": ["run_quality_check"],
        "always": [
            {"target": "ready", "guard": "quality_passed"},
            {"target": "remaking", "guard": "quality_failed", "actions": ["log_quality_fail"]},
        ],
    },
    "remaking": {
        "entry": ["reset_for_remake"],
        "always": [{"target": "preparing"}],
    },
    "ready": {
        "entry": ["notify_ready"],
        "on": {"DISPATCH": {"target": "assigning_driver"}},
    },
    "assigning_driver": {
        "entry": ["find_driver"],
        "always": [
            {"target": "out_for_delivery", "guard": "driver_found", "actions": ["notify_dispatch"]},
        ],
    },
    "out_for_delivery": {
        "on": {
            "DELIVERED": {"target": "delivered", "actions": ["complete_delivery"]},
            "FAILED": {"target": "delivery_failed", "actions": ["handle_delivery_failure"]},
        },
        "error_state": "delivery_failed",
    },
    "delivered": {},
    "delivery_failed": {
        "on": {
            "RETRY_DELIVERY": {"target": "assigning_driver", "actions": ["reset_driver"]},
            "CANCEL": {"target": "cancelled", "actions": ["notify_cancellation"]},
        }
    },
    "cancelled": {},
}

ACTIONS = {
    "create_order": create_order,
    "notify_received": notify_received,
    "charge_card": charge_card,
    "notify_payment_success": notify_payment_success,
    "notify_payment_failure": notify_payment_failure,
    "reset_payment": reset_payment,
    "start_preparation": start_preparation,
    "log_prep_complete": log_prep_complete,
    "start_baking": start_baking,
    "run_quality_check": run_quality_check,
    "log_quality_fail": log_quality_fail,
    "reset_for_remake": reset_for_remake,
    "notify_ready": notify_ready,
    "find_driver": find_driver,
    "notify_dispatch": notify_dispatch,
    "complete_delivery": complete_delivery,
    "handle_delivery_failure": handle_delivery_failure,
    "reset_driver": reset_driver,
    "notify_cancellation": notify_cancellation,
}

GUARDS = {
    "payment_approved": payment_approved,
    "payment_declined": payment_declined,
    "prep_complete": prep_complete,
    "quality_passed": quality_passed,
    "quality_failed": quality_failed,
    "driver_found": driver_found,
}

# --- Demo runs ---


async def run_happy_path_with_quality_fail() -> None:
    """Happy path: Card payment, quality fails once -> remake -> pass -> delivered."""
    print("\n" + "=" * 60)
    print("RUN 1 - Happy path (quality fail -> remake loop)")
    print("=" * 60)

    machine = StateMachine.from_dict(CONFIG, action_dict=ACTIONS, guard_dict=GUARDS)
    session = PizzaSession(customer_name="Alice")
    state = "order_received"
    cursor = 0

    signals = [
        (
            "CONFIRM",
            {"customer_name": "Alice", "pizza": "Pepperoni", "address": "42 Baker St", "payment_method": "card"},
        ),
        ("BAKING_DONE", {}),  # first bake -> quality fail -> remake
        ("BAKING_DONE", {}),  # second bake -> quality pass -> ready
        ("DISPATCH", {}),
        ("DELIVERED", {}),
    ]

    for event, data in signals:
        state = await machine.run(
            state_name=state,
            events=Signal(event, data),
            session=session,
        )
        new_notes = session.notes[cursor:]
        cursor = len(session.notes)
        print(f"\n[{event}] -> {state}")
        for note in new_notes:
            print(f"  {note}")

    print(f"\n Final: {state} | order {session.order_id} | receipt via driver {session.driver_id}")


async def run_payment_declined_retry() -> None:
    """Payment declined on cash, retried with card, then delivered."""
    print("\n" + "=" * 60)
    print("RUN 2 - Payment declined -> retry with card")
    print("=" * 60)

    machine = StateMachine.from_dict(CONFIG, action_dict=ACTIONS, guard_dict=GUARDS)
    session = PizzaSession(customer_name="Bob")
    state = "order_received"
    cursor = 0

    signals = [
        ("CONFIRM", {"customer_name": "Bob", "pizza": "Margherita", "address": "7 Elm Rd", "payment_method": "cash"}),
        ("RETRY_PAYMENT", {"payment_method": "card"}),
        ("BAKING_DONE", {}),
        ("BAKING_DONE", {}),  # quality fail -> remake path again
        ("DISPATCH", {}),
        ("DELIVERED", {}),
    ]

    for event, data in signals:
        state = await machine.run(
            state_name=state,
            events=Signal(event, data),
            session=session,
        )
        new_notes = session.notes[cursor:]
        cursor = len(session.notes)
        print(f"\n[{event}] -> {state}")
        for note in new_notes:
            print(f"  {note}")

    print(f"\n Final: {state} | order {session.order_id}")


async def run_streaming_demo() -> None:
    """Show raw AG-UI event stream for one signal turn."""
    print("\n" + "=" * 60)
    print("RUN 3 - AG-UI stream() events for CONFIRM signal")
    print("=" * 60)

    machine = StateMachine.from_dict(CONFIG, action_dict=ACTIONS, guard_dict=GUARDS)
    session = PizzaSession(customer_name="Carol")

    async for event in machine.stream(
        state_name="order_received",
        events=Signal(
            "CONFIRM",
            {"customer_name": "Carol", "pizza": "BBQ Chicken", "address": "99 Pine Ave", "payment_method": "card"},
        ),
        session=session,
        run_id="pizza-stream-demo",
        thread_id="thread-carol-001",
    ):
        print(f" {event.type.value:<28} {_event_summary(event)}")


def _event_summary(event: BaseEvent) -> str:
    from ag_ui.core import EventType  # noqa: PLC0415 -- lazy so plain `import statem` never needs ag-ui-protocol

    if event.type == EventType.STATE_SNAPSHOT:
        return f"snapshot={event.snapshot}"
    if event.type == EventType.STEP_STARTED:
        return f"step={event.step_name}"
    if event.type == EventType.STEP_FINISHED:
        return f"step={event.step_name}"
    if event.type == EventType.ACTIVITY_SNAPSHOT:
        return event.content
    if event.type == EventType.STATE_DELTA:
        return str([f"{p['op']} {p['path']}={p['value']}" for p in event.delta])
    return ""


async def main() -> None:
    print(to_mermaid(StateMachine.from_dict(CONFIG, action_dict=ACTIONS, guard_dict=GUARDS), initial="order_received"))
    await run_happy_path_with_quality_fail()
    await run_payment_declined_retry()
    await run_streaming_demo()


if __name__ == "__main__":
    asyncio.run(main())
