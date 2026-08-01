# Guide

## Config shape

`StateMachine.from_dict(config, action_dict=None, guard_dict=None)` takes a plain dict mapping
state name → state config. Each state config accepts:

| Field | Meaning |
|---|---|
| `on` | `{event_name: transition(s)}` — candidates tried in order; the first whose guard passes fires. |
| `always` | Eventless transition candidates, re-checked after every state entry. |
| `entry` | Action names to run when the state is entered. |
| `exit` | Action names to run when the state is left. |
| `error_state` | Fallback state to enter if an `on`-transition's action raises. |

A transition candidate is `{"target": ..., "guard": ... (optional), "actions": [...] (optional)}`.
Shorthand is normalized automatically:

```python
"on": {"START": "running"}                                    # -> target only
"on": {"START": {"target": "running", "guard": "can_start"}}   # -> single dict
"on": {"START": ["blocked", {"target": "running", "guard": "g"}]}  # -> list of candidates
```

`"*"` is a wildcard event — used when no exact event match exists for the current state.

Config is validated eagerly: unknown transition targets (in `on`, `always`, or `error_state`)
raise at construction time, and if `action_dict`/`guard_dict` are supplied, every referenced
guard/action name must be registered too.

## Guards and actions

Both are plain Python callables — sync or async — taking `(ctx, signal)`:

```python
def can_start(ctx, signal) -> bool: ...          # guards must return bool
async def create_order(ctx, signal) -> None: ...  # actions may return anything (or nothing)
```

Register them via `from_dict(config, action_dict={...}, guard_dict={...})`, or later through
`machine.actions.register(name, fn)` / `machine.guards.register(name, fn)`.

## Running the machine

```python
state = await machine.run(state_name, events, session, run_id=None)
```

- `events` — a single `Signal`, a list of `Signal`s, or `[]` to only resolve pending
  `always`-transitions for the current state.
- `session` — **any object you want.** The engine never inspects or mutates its shape; it's
  simply attached to the per-run `ExecutionContext` (`ctx.session`) so your guards/actions can
  read and mutate whatever they need.
- `run_id` — an optional correlation id used in log lines (`run_id=... | state=... | ...`).
  Auto-generated via `uuid4().hex` if omitted.
- Returns the final state name once every signal has been processed and all pending
  `always`-transitions have settled.

## `always`-transitions

After every state entry (including the initial state, before any signal is processed), the
engine checks that state's `always` list. If a guard passes, its actions run, the machine enters
the target, and the check repeats from there — so one `run()` call can auto-advance through
several states in a row. If no guard passes, nothing happens. This loop is capped at 100 hops; an
`always` chain that never settles raises `RuntimeError`.

## `error_state` fallback

If an `on`-transition's action raises, the engine wraps it as `TransitionError`. If the current
state declares `error_state`, the machine transitions there instead of raising (a warning is
logged); otherwise the `TransitionError` propagates to the caller.

## Tracing a run

Every guard/action evaluated during a `run()` call is recorded, in order, on
`ctx.results` (a list of `ResultEntry`). [`show_transitions(ctx)`](api.md#statem.show_transitions)
renders that trace as a readable hop-by-hop report — useful for debugging why a machine ended up
where it did. Note `run()` itself only returns the final state name; to inspect the trace, work
with an `ExecutionContext` directly (as the test suite does) or rely on the `run_id`-tagged log
output.

## Further reading

The [API Reference](api.md) documents every public class and function directly from source.
