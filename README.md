# statem

A minimal async state machine engine for Python, built on [Pydantic](https://docs.pydantic.dev/).
State/transition config is a plain dict, validated by Pydantic at construction time; guard and
action *behavior* is ordinary Python (sync or async) code, registered by name. Ships a `py.typed`
marker and is tested to 100% branch coverage (see [Development](#development)).

Full documentation: <https://rahuldas-dev.github.io/statem/>

## Philosophy

**The state graph is data; behavior is code — and they're kept apart.** `config` is a plain,
JSON/YAML-friendly dict validated by Pydantic. It can be authored by hand, generated, loaded from
a file or a database, or produced by an LLM — the engine doesn't care where it came from. Guards
and actions are ordinary Python functions, registered by name and swapped in independently of the
graph. Neither side needs to know about the other's implementation details.

**Fail fast, not deep in production.** Every transition target, and (if you supply
`action_dict`/`guard_dict`) every referenced guard/action name, is validated at construction
time — a typo in your config raises immediately, not three hops into a live run.

**Async where it matters, sync where it doesn't.** Guards and actions may be sync or async in any
combination; the engine detects and awaits each correctly, so you're never forced to wrap trivial
sync logic in `async def` just to satisfy the type system.

**The engine owns transitions, not your data.** `session` is opaque — a dict, a dataclass, an ORM
row, anything. The machine never inspects or reshapes it; it's simply threaded through to your
guards/actions via `ctx.session`. `statem` is not a state-storage or persistence layer, and
doesn't try to be one.

**No subclassing, no base classes.** A `StateMachine` is a plain object built with
`StateMachine.from_dict(...)` and driven with `await machine.run(...)`. There's no framework
lifecycle to inherit into and no required web/ORM integration.

**Every run is traceable.** Each guard/action evaluated during a `run()` call is recorded in
order (accessible via a directly-built `ExecutionContext`, see the [guide](https://rahuldas-dev.github.io/statem/guide/)),
and a `run_id` (yours, or an auto-generated one) ties every log line from one run together —
because in practice, reconstructing *why* a machine ended up in a given state is the actual hard
part.

## Hooks

Four lifecycle hooks can carry guards and/or actions on any state:

| Hook | Fires when | Carries |
|---|---|---|
| `on` | An external `Signal` (event) is dispatched and matches this state's `on` map (or its `"*"` wildcard) | Candidates tried in order: `guard` (optional, must return `bool`) gates the candidate; its `actions` run if it fires |
| `always` | Automatically, right after *any* state entry (including the initial one) — no external signal needed | Same shape as `on`, minus the event name; used to auto-advance once a condition becomes true |
| `entry` | A state is entered (after the firing transition's own `actions`) | Action names only |
| `exit` | A state is left, before entering the next one | Action names only |

A fifth field, `error_state`, isn't a hook itself but a per-state escape hatch: if an
`on`-transition's action raises, the engine catches it and — if `error_state` is set — transitions
there instead of propagating the exception.

`always` re-checks after every entry it causes, so a single `run()` call can chain through several
states automatically (capped at 100 hops, to catch runaway loops). See the
[guide](https://rahuldas-dev.github.io/statem/guide/) for the full config shape and shorthand
forms.

## Install

```bash
pip install statem
```

The importable package is `statem`:

```python
from statem import StateMachine, Signal
```

## Quickstart

```python
import asyncio
from statem import Signal, StateMachine

config = {
    "idle": {"on": {"START": {"target": "running", "guard": "can_start"}}},
    "running": {"on": {"STOP": "idle"}},
}


def can_start(ctx, signal) -> bool:
    return True


async def main() -> None:
    machine = StateMachine.from_dict(config, guard_dict={"can_start": can_start})
    state = await machine.run(state_name="idle", events=Signal(event="START"), session={})
    print(state)  # "running"


asyncio.run(main())
```

See `examples/baking/example.py` for a fuller example (guards, actions, `error_state`, and an
`always`-transition), and the [guide](https://rahuldas-dev.github.io/statem/guide/) for a
full walkthrough of the config shape.

## Development

```bash
uv sync
uv run python -m unittest discover -s tests
uv run coverage run -m unittest discover -s tests && uv run coverage report --fail-under=100
uv run ruff check tests statem
```

## License

MIT — see [LICENSE](LICENSE).
