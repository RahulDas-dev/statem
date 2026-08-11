from __future__ import annotations

import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .machine import StateMachine

_UNSAFE_CHARS = re.compile(r"[^0-9A-Za-z_]")


def to_mermaid(machine: StateMachine, *, initial: str | None = None) -> str:
    """Render `machine.config` as a Mermaid `stateDiagram-v2` diagram source string.

    One edge is emitted per transition candidate: `on` candidates are labeled with the event
    name (plus `[guard_name]` if guarded), `always` candidates are labeled `always` (plus the
    guard, if any), and each `error_state` becomes an `error`-labeled edge. A guard-chain --
    multiple candidates for one event -- naturally produces multiple labeled edges from the same
    state, which is the main payoff: it visualizes branching that a flat transition table hides.

    Paste the result into a Markdown "mermaid" code fence (GitHub, Sphinx, VS Code, and Jupyter
    all render it natively) to view the diagram.

    Args:
        machine: The `StateMachine` whose `config` to render.
        initial: If given and present in `machine.config`, prepends a `[*] --> initial` edge
            marking the diagram's entry point. `StateMachine` itself has no notion of an
            "initial" state -- that's chosen fresh by the caller on every `run()` call -- so
            this is opt-in.

    Returns:
        Mermaid `stateDiagram-v2` source, ready to paste into a fence.
    """
    ids = _assign_ids(machine.config)
    lines = ["stateDiagram-v2"]

    for name, safe_id in ids.items():
        if safe_id != name:
            lines.append(f'    state "{name}" as {safe_id}')

    if initial is not None and initial in ids:
        lines.append(f"    [*] --> {ids[initial]}")

    for name, state_cfg in machine.config.items():
        src = ids[name]

        for event, candidates in state_cfg.on.items():
            for candidate in candidates:
                label = event if candidate.guard is None else f"{event} [{candidate.guard}]"
                lines.append(f"    {src} --> {ids[candidate.target]}: {label}")

        for candidate in state_cfg.always:
            label = "always" if candidate.guard is None else f"always [{candidate.guard}]"
            lines.append(f"    {src} --> {ids[candidate.target]}: {label}")

        if state_cfg.error_state is not None:
            lines.append(f"    {src} --> {ids[state_cfg.error_state]}: error")

    return "\n".join(lines)


def _assign_ids(config: dict[str, object]) -> dict[str, str]:
    """Map each state name to a safe, unique Mermaid id, preserving declaration order."""
    ids: dict[str, str] = {}
    used: set[str] = set()

    for name in config:
        safe = _UNSAFE_CHARS.sub("_", name)
        if not safe or safe[0].isdigit():
            safe = f"s_{safe}"

        candidate = safe
        suffix = 2
        while candidate in used:
            candidate = f"{safe}_{suffix}"
            suffix += 1

        ids[name] = candidate
        used.add(candidate)

    return ids
