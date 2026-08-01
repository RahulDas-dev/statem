"""statem - minimal async state machine framework.

Public API re-exported from this package so callers can import directly:

    from statem import StateMachine, Signal, ExecutionContext
"""

from .machine import StateMachine
from .schema import (
    ActionFn,
    ActionRegistry,
    ExecutionContext,
    GuardError,
    GuardFn,
    GuardRegistry,
    ResultEntry,
    Signal,
    StateConfig,
    TransitionConfig,
    TransitionError,
)
from .utility import show_transitions

__all__ = [
    "ActionFn",
    "ActionRegistry",
    "ExecutionContext",
    "GuardError",
    "GuardFn",
    "GuardRegistry",
    "ResultEntry",
    "Signal",
    "StateConfig",
    "StateMachine",
    "TransitionConfig",
    "TransitionError",
    "show_transitions",
]
