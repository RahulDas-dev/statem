"""statem - minimal async state machine framework.

Public API re-exported from this package so callers can import directly:

    from statem import StateMachine, Signal, Context
"""

from .diagram import to_mermaid
from .machine import StateMachine
from .schema import (
    ActionFn,
    ActionRegistry,
    Context,
    GuardError,
    GuardFn,
    GuardRegistry,
    ResultEntry,
    Signal,
    StateConfig,
    TransitionConfig,
    TransitionError,
)
from .tracing import show_transitions

__all__ = [
    "ActionFn",
    "ActionRegistry",
    "Context",
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
    "to_mermaid",
]
