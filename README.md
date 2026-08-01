# statem

A minimal async state machine engine for Python, built on [Pydantic](https://docs.pydantic.dev/).
State/transition config is a plain dict, validated by Pydantic at construction time; guard and
action *behavior* is ordinary Python (sync or async) code, registered by name.

- **Declarative config** — states, transitions, guards, and actions declared as data (JSON/YAML-friendly), not hardcoded classes.
- **Async-first engine** — `guards` and `actions` may be sync or async; the machine `await`s either transparently.
- **`always`-transitions** — eventless, auto-advancing transitions checked after every state entry.
- **`error_state` fallback** — an unhandled action error can transition to a designated recovery state instead of raising.
- **Session-agnostic** — `session` is any object you want (dict, dataclass, ORM row); the engine never inspects it, only threads it through to your guards/actions.
- **Fully typed & tested** — ships a `py.typed` marker; 100% branch coverage.

Full documentation: <https://rahuldas-dev.github.io/statemachine/>

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
    state = await machine.run("idle", Signal(event="START"), session={})
    print(state)  # "running"


asyncio.run(main())
```

See `examples/baking/example.py` for a fuller example (guards, actions, `error_state`, and an
`always`-transition), and the [guide](https://rahuldas-dev.github.io/statemachine/guide/) for a
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
