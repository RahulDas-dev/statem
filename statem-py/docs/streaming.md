# Streaming (AG-UI events)

`StateMachine.stream(...)` drives the exact same engine as [`run()`](guide.md#running-the-machine)
— same guards, same actions, same transition rules, same effect on `session` — but instead of
returning only the final state name, it's an async generator yielding
[AG-UI protocol](https://docs.ag-ui.com/) events as the machine executes. Use it when a caller
(a chat UI, an SSE endpoint, a CLI progress view) needs to observe *how* the machine got to its
final state, not just the destination.

`run()` itself is completely unaffected — `stream()` is purely additive.

## Install

`stream()` requires the `agui` extra:

```bash
pip install statem[agui]
```

Plain `import statem` never needs `ag-ui-protocol` or `jsonpatch` — they're only imported the
moment `stream()` is actually called. Calling it without the extra installed raises `ImportError`
with that install instruction.

## Usage

```python
from statem import Signal, StateMachine

config = {
    "idle": {"on": {"START": {"target": "running", "guard": "can_start"}}},
    "running": {},
}
machine = StateMachine.from_dict(config, guard_dict={"can_start": lambda ctx, signal: True})

async for event in machine.stream(state_name="idle", events=Signal(event="START"), session={}):
    print(event.type, event)
```

`stream()` takes the same keyword-only arguments as `run()` (`run_id`, `thread_id`, `state_name`,
`events`, `session`), plus one more:

- `state_accessor` — an optional `Callable[[session], dict]` that derives the dict broadcast via
  `STATE_SNAPSHOT`/`STATE_DELTA` from `session` (e.g. `lambda session: session.to_dict()`).
  Called at the start and end of every step. Defaults to `{"current_state": <state name>}` when
  omitted, so it works out of the box even for an opaque `session`.

## Event sequence

A **step** is one state change (one hop) — whether triggered by an `on`-transition, an `always`
cascade hop, or an `error_state` fallback — not one call to `stream()`. A single signal that
triggers an `always` cascade produces multiple steps, one per hop. Each step is fully
self-contained, emitted in this order:

1. **`STEP_STARTED`** — `step_name` is the triggering signal's event name, or `"__always__"` for
   an `always`-cascade hop.
2. **`STATE_SNAPSHOT`** — taken right before this hop's guards/actions run.
3. **`ACTIVITY_SNAPSHOT`** — one per guard/action result, in firing order, as they happen during
   this hop. A guard that's evaluated but doesn't fire (e.g. an earlier candidate in a guard
   chain) is still reported, just before its step opens. `content` is
   `{"type": "activity", "kind": "guard" | "action", "name": ..., "source": ..., "result": ...}`
   — `source` is the lifecycle hook (`"on"`, `"always"`, `"entry"`, `"exit"`); `result` is the
   guard's `bool` for a guard, the action's return value (or `None`) for an action.
4. **`STATE_DELTA`** — an RFC 6902 JSON Patch (via `jsonpatch.make_patch`) between this step's own
   `STATE_SNAPSHOT` and the state right before `STEP_FINISHED`. **Skipped entirely** if nothing
   the accessor reports actually changed during this step.
5. **`STEP_FINISHED`**.

A signal that matches no transition produces no events at all. A signal whose guard(s) all fail
still opens and closes a step (with guard `ACTIVITY_SNAPSHOT`s, but no `STATE_DELTA`, since
nothing changed).

`RUN_STARTED` / `RUN_FINISHED` / `RUN_ERROR` are **never** emitted — the caller starting/ending
iteration over the async generator already marks the run's boundaries. An unhandled exception
(e.g. an action error with no `error_state` configured) propagates out of the generator exactly
as it would out of `run()` — there's no `RUN_ERROR` substitute swallowing it.

## Worked example

```{literalinclude} ../examples/pizza.py
:language: python
:pyobject: run_streaming_demo
```
