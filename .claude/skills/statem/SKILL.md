---
name: statem
description: Build or modify async state machines using the statem library (pip install statem, import statem). Use whenever the user wants to model a workflow, conversation/bot flow, approval pipeline, order/transaction lifecycle, or any process with discrete named states and transitions -- especially if the project already depends on statem, or the user mentions StateMachine, guards, transitions, or "state machine" in a Python context.
---

# Building state machines with `statem`

`statem` is a minimal async state machine engine built on Pydantic. The state graph is a plain
dict, validated eagerly; guard/action *behavior* is ordinary Python functions (sync or async)
registered by name. Full docs: <https://rahuldas-dev.github.io/statem/>.

## Core shape

```python
from statem import StateMachine, Signal

config = {
    "idle": {"on": {"START": {"target": "running", "guard": "can_start"}}},
    "running": {"on": {"STOP": "idle"}},
}

def can_start(ctx, signal) -> bool:
    return True

machine = StateMachine.from_dict(config, guard_dict={"can_start": can_start})
state = await machine.run(state_name="idle", events=Signal(event="START"), session={})
```

Each state config accepts five fields -- these are the only things a state can declare:

| Field | Meaning |
|---|---|
| `on` | `{event_name: transition(s)}`. Candidates tried in order; first guard that passes fires. `"*"` is a wildcard event, used when no exact match exists. |
| `always` | Eventless transition candidates, re-checked after *every* state entry (including the initial one, before any signal). No event needed. |
| `entry` | Action names to run when the state is entered. |
| `exit` | Action names to run when the state is left. |
| `error_state` | Fallback state if an `on`-transition's action raises. |

A transition candidate is `{"target": ..., "guard": ... (optional), "actions": [...] (optional)}`.
Shorthand is normalized automatically -- `"START": "running"` is equivalent to
`"START": {"target": "running"}`, and a list makes a guard chain:
```python
"on": {"START": [
    {"target": "running", "guard": "can_start"},
    {"target": "blocked", "guard": "cannot_start"},
]}
```

Guards and actions are plain callables taking `(ctx, signal)`, sync or async, mixed freely:
```python
def guard_fn(ctx, signal) -> bool: ...       # MUST return bool, or GuardError is raised
async def action_fn(ctx, signal) -> Any: ... # may return anything, or nothing
```
Register via `from_dict(config, action_dict={...}, guard_dict={...})`, or later via
`machine.actions.register(name, fn)` / `machine.guards.register(name, fn)` followed by
`machine.validate_registries()` to catch typos (raises `ValueError` listing anything missing).

## Running

```python
state = await machine.run(run_id=None, state_name="idle", events=signal_or_list_or_empty, session=my_session)
```
**All arguments to `run()` are keyword-only** (`state_name=`, `events=`, `session=`, `run_id=` --
never positional). `events` can be a single `Signal`, a list of `Signal`s, or `[]` to only resolve
pending `always`-transitions. `run_id` is an optional correlation id for log lines
(`run_id=... | state=... | ...`); omit it and a `uuid4().hex` is generated automatically.
`run()` returns only the final state name (a `str`) -- nothing else.

`session` is **fully opaque** -- a dict, dataclass, ORM row, whatever the app needs. The engine
never inspects or reshapes it, only threads it through to `ctx.session`. For typed access inside
guards/actions, annotate with the generic `Context`:
```python
from statem import Context, Signal
from dataclasses import dataclass

@dataclass
class MySession:
    amount: float

def my_guard(ctx: Context[MySession], signal: Signal) -> bool:
    return ctx.session.amount > 100  # ctx.session is typed as MySession here
```

## Gotchas (read before debugging something weird)

- **`error_state` only catches exceptions from `on`-transition actions.** It does *not* catch
  exceptions raised inside `entry`/`exit` actions, or inside `always`-transition actions -- those
  propagate as raw, unhandled exceptions. If an action might fail and you want recovery, attach it
  to an `on` transition's `actions` list, not to `entry`/`exit`/`always`.
- **Guards must return exactly `bool`.** Returning a truthy/falsy non-bool (e.g. `None`, `0`, a
  string) raises `GuardError`, it does not get coerced.
- **`always` auto-advances after every state entry**, including the very first one before any
  signal is processed. A single `run()` call can chain through several states automatically if
  each new state's `always` guard passes. This loop is capped at 100 hops; a chain that never
  settles raises `RuntimeError`.
- **Construction validates eagerly.** Bad transition targets (in `on`, `always`, or `error_state`)
  raise at `StateMachine` construction time. If you pass `action_dict`/`guard_dict` to
  `from_dict`, every referenced guard/action name must already be registered too, or it raises
  then -- not later when the transition actually tries to fire.
- **`run()` only returns the final state name**, not a trace. To inspect what fired during a run,
  build a `Context` directly and call the lower-level pieces (see the test suite for the pattern),
  or use `show_transitions(ctx)` for a human-readable hop-by-hop report, or rely on the
  `run_id`-tagged log output.

## Visualizing

`to_mermaid(machine, initial=None)` renders `machine.config` as a Mermaid `stateDiagram-v2`
string -- paste it into a ` ```mermaid ` fence (GitHub, MkDocs, VS Code, Jupyter all render it
natively). Useful for sanity-checking a graph you just built, especially guard chains, before
wiring up actual guard/action functions.

## When building a new state machine for the user

1. List the states and the events/conditions that move between them before writing any config --
   confirm the graph with the user (or via `to_mermaid`) if it's non-trivial.
2. Prefer `always` for conditions the system itself resolves (no user input needed) and `on` for
   things an external actor triggers.
3. Only reach for `error_state` on `on`-transitions where the action can genuinely fail (e.g. a
   network call) -- see the gotcha above.
4. Keep guard/action functions small and named for what they check/do, not for the state they're
   attached to -- they're registered globally by name and can be reused across transitions.
5. Write a runnable driver (loop calling `run()` per incoming event, threading `state` and
   `session` through) rather than a single giant `run()` call with every signal pre-scripted,
   unless the whole sequence really is known upfront.
