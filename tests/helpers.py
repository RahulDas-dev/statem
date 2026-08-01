from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from statem.schema import ExecutionContext, Signal


def make_session() -> dict[str, Any]:
    """An example opaque session payload -- its shape is not enforced by the engine."""
    return {"app": {}, "user": {}}


def sync_action(ctx: ExecutionContext, signal: Signal) -> None:
    return None


async def async_action(ctx: ExecutionContext, signal: Signal) -> None:
    return None


def action_returns_value(ctx: ExecutionContext, signal: Signal) -> Any:
    return "action-result"


def action_raises(ctx: ExecutionContext, signal: Signal) -> None:
    raise ValueError("boom")


def sync_guard_true(ctx: ExecutionContext, signal: Signal) -> bool:
    return True


def sync_guard_false(ctx: ExecutionContext, signal: Signal) -> bool:
    return False


async def async_guard_true(ctx: ExecutionContext, signal: Signal) -> bool:
    return True


async def async_guard_false(ctx: ExecutionContext, signal: Signal) -> bool:
    return False


def guard_non_bool(ctx: ExecutionContext, signal: Signal) -> str:
    return "not-a-bool"
