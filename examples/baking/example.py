"""Runnable example: a bakery process modeled as a StateMachine.

Demonstrates:
- declaring config as a plain dict (on / always / entry / error_state)
- registering guards and actions by name
- an arbitrary, library-agnostic ``session`` object (here: a plain dataclass)
- the ``always``-transition auto-advance mechanism
"""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field

from statem import ExecutionContext, Signal, StateMachine

logging.basicConfig(level=logging.INFO, format="%(message)s")


@dataclass
class BakingSession:
    oven_temp_c: int = 0
    ingredients_checked: bool = False
    log: list[str] = field(default_factory=list)


def check_ingredients(ctx: ExecutionContext, signal: Signal) -> None:
    ctx.session.ingredients_checked = True
    ctx.session.log.append("ingredients checked")


def ingredients_ready(ctx: ExecutionContext, signal: Signal) -> bool:
    return ctx.session.ingredients_checked


def preheat_oven(ctx: ExecutionContext, signal: Signal) -> None:
    ctx.session.oven_temp_c = 180
    ctx.session.log.append("oven preheated to 180C")


def start_cooling(ctx: ExecutionContext, signal: Signal) -> None:
    ctx.session.oven_temp_c = 25
    ctx.session.log.append("cake pulled, cooling started")


def oven_is_cool(ctx: ExecutionContext, signal: Signal) -> bool:
    return ctx.session.oven_temp_c <= 30


def plate_cake(ctx: ExecutionContext, signal: Signal) -> None:
    ctx.session.log.append("cake plated")


CONFIG = {
    "idle": {
        "on": {"START": {"target": "mixing", "actions": ["check_ingredients"]}},
    },
    "mixing": {
        "on": {"MIXED": {"target": "baking", "guard": "ingredients_ready", "actions": ["preheat_oven"]}},
        "error_state": "failed",
    },
    "baking": {
        "on": {"TIMER_DONE": {"target": "cooling", "actions": ["start_cooling"]}},
    },
    "cooling": {
        "always": [{"target": "done", "guard": "oven_is_cool"}],
    },
    "done": {
        "entry": ["plate_cake"],
    },
    "failed": {},
}


async def main() -> None:
    machine = StateMachine.from_dict(
        CONFIG,
        action_dict={
            "check_ingredients": check_ingredients,
            "preheat_oven": preheat_oven,
            "start_cooling": start_cooling,
            "plate_cake": plate_cake,
        },
        guard_dict={
            "ingredients_ready": ingredients_ready,
            "oven_is_cool": oven_is_cool,
        },
    )

    session = BakingSession()
    state = "idle"
    for event in ("START", "MIXED", "TIMER_DONE"):
        state = await machine.run(
            run_id="bake-001",
            state_name=state,
            events=Signal(event=event),
            session=session,
        )

    print(f"final state: {state}")
    print("session log:")
    for line in session.log:
        print(f"  - {line}")


if __name__ == "__main__":
    asyncio.run(main())
