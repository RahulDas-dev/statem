# Quickstart

## A minimal machine

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
    print(state)


asyncio.run(main())
```

Output:

```text
running
```

Walking through what happened:

1. `StateMachine.from_dict(config, guard_dict={...})` validates `config` against the
   [`StateConfig`](api.md#statem.StateConfig) schema, registers the `can_start` guard, and
   checks every guard/action referenced in `config` is registered (since `guard_dict` was
   supplied).
2. `machine.run(state_name="idle", events=Signal(event="START"), session={})` starts in `"idle"`, dispatches a
   `START` signal, evaluates the `can_start` guard, and — since it passes — transitions to
   `"running"`.
3. The final state name is returned as a plain string.

## A fuller example

[`examples/baking/example.py`](https://github.com/RahulDas-dev/statem/blob/main/examples/baking/example.py)
in the repository builds a small bakery process (`idle → mixing → baking → cooling → done`) that
exercises guards, actions, an `error_state` fallback, and an `always`-transition that
auto-advances `cooling → done` once the oven has cooled — all triggered by a single `TIMER_DONE`
signal. Run it with:

```bash
uv run python examples/baking/example.py
```

See the [Guide](guide.md) for what each config field (`on`, `always`, `entry`, `exit`,
`error_state`) does, and the [API Reference](api.md) for the full public surface.
