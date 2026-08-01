# statem

A minimal async state machine engine for Python, built on [Pydantic](https://docs.pydantic.dev/).
State/transition config is a plain dict, validated by Pydantic at construction time; guard and
action *behavior* is ordinary Python (sync or async) code, registered by name.

- **Declarative config** — states, transitions, guards, and actions declared as data (JSON/YAML-friendly), not hardcoded classes.
- **Async-first engine** — guards and actions may be sync or async; the machine `await`s either transparently.
- **`always`-transitions** — eventless, auto-advancing transitions checked after every state entry.
- **`error_state` fallback** — an unhandled action error can transition to a designated recovery state instead of raising.
- **Session-agnostic** — `session` is any object you want (dict, dataclass, ORM row); the engine never inspects it, only threads it through to your guards/actions.
- **Fully typed & tested** — ships a `py.typed` marker; 100% branch coverage.

## Install

```bash
pip install statem
```

The importable package is `statem`:

```python
from statem import StateMachine, Signal
```

Continue to the [Quickstart](quickstart.md) for a runnable example, or the [Guide](guide.md) for
a full walkthrough of the config shape and execution model.
