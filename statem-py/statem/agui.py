"""AG-UI protocol event builders used by `StateMachine.stream`.

Imported lazily from `machine.py` (only when `stream()` is actually called) so that plain
`import statem` never requires the `ag-ui-protocol`/`jsonpatch` packages. Install with `pip
install statem[agui]` to use `stream()`.
"""

from __future__ import annotations

import uuid
from typing import TYPE_CHECKING, Any

try:
    import jsonpatch
    from ag_ui.core import (
        ActivitySnapshotEvent,
        StateDeltaEvent,
        StateSnapshotEvent,
        StepFinishedEvent,
        StepStartedEvent,
    )
except ImportError as exc:
    raise ImportError(
        "StateMachine.stream() requires the 'ag-ui-protocol' and 'jsonpatch' packages. "
        "Install them with: pip install statem[agui]"
    ) from exc

if TYPE_CHECKING:
    from .schema import ResultEntry


def state_snapshot(state: dict[str, Any]) -> StateSnapshotEvent:
    """`STATE_SNAPSHOT` emitted once, at the beginning of a stream."""
    return StateSnapshotEvent(snapshot=state)


def diff_state(last_snapshot: dict[str, Any], current_state: dict[str, Any]) -> list[dict[str, Any]]:
    """RFC 6902 patch turning *last_snapshot* into *current_state*, via `jsonpatch.make_patch`."""
    return jsonpatch.make_patch(last_snapshot, current_state).patch


def state_delta(patch: list[dict[str, Any]]) -> StateDeltaEvent:
    """`STATE_DELTA` emitted each time the machine settles into a new state."""
    return StateDeltaEvent(delta=patch)


def step_started(signal_event: str) -> StepStartedEvent:
    """`STEP_STARTED` emitted before a signal is pushed into the machine."""
    return StepStartedEvent(step_name=signal_event)


def step_finished(signal_event: str) -> StepFinishedEvent:
    """`STEP_FINISHED` emitted after a signal has fully settled (including any `always` cascade)."""
    return StepFinishedEvent(step_name=signal_event)


def activity(entry: ResultEntry) -> ActivitySnapshotEvent:
    """`ACTIVITY_SNAPSHOT` for a single guard or action result: kind, name, hook source, and value."""
    return ActivitySnapshotEvent(
        message_id=uuid.uuid4().hex,
        activity_type=entry.kind,
        content={
            "type": "activity",
            "kind": entry.kind,
            "name": entry.name,
            "source": entry.source,
            "result": entry.value,
        },
    )
