from __future__ import annotations

from string import Template
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from .schema import Context

_DIVIDER: Final[str] = "--------------------------------------------------------------------------------"
_SRC_W: Final[int] = 6
_NAME_WIDTH: Final[int] = 25

_TRANSITION_TEMPLATE: Final[Template] = Template("Transitions ($hops hop$plural):\n$divider\n$body")

_HOP_BLOCK_TEMPLATE: Final[Template] = Template("hop $n: $from_state -> $to_state\n$content$divider\n")

_FINAL_BLOCK_TEMPLATE: Final[Template] = Template("Final state: $state\n$content$divider")


def show_transitions(ctx: Context) -> str:
    """Return a human-readable summary of every transition in execution order.

    Iterates `ctx.results` (a `list[ResultEntry]`) which preserves the
    exact order guards and actions fired. Each hop groups entries by state.

    Example output:
        ```text
        Transitions (2 hops):
        --------------------------------------------------------------------------------
        hop 1: idle -> collecting
            always guard : session_limit_reached      = False
            on     guard : can_start_txn              = True
                   action : create_txn                = None
                   action : resolve_fields            = None
        --------------------------------------------------------------------------------
        hop 2: collecting -> confirming
            always guard : is_enquiry_ready           = False
                   guard : all_resolved_and_shown     = False
                   guard : all_fields_resolved        = True
            exit   action : mark_shown                = None
        --------------------------------------------------------------------------------
        Final state: confirming
            entry  action : show_confirm              = None
        --------------------------------------------------------------------------------
        ```
    """
    hops = len(ctx.history) - 1
    return _TRANSITION_TEMPLATE.substitute(
        hops=hops,
        plural="s" if hops != 1 else "",
        divider=_DIVIDER,
        body=_render_body(ctx),
    )


def _render_body(ctx: Context) -> str:
    """Build the per-hop content that fills `$body`."""
    blocks: list[str] = []

    for i, state in enumerate(ctx.history):
        content = _format_state_section(state, ctx)
        if i < len(ctx.history) - 1:
            blocks.append(
                _HOP_BLOCK_TEMPLATE.substitute(
                    n=i + 1,
                    from_state=state,
                    to_state=ctx.history[i + 1],
                    content=content,
                    divider=_DIVIDER,
                )
            )
        else:
            blocks.append(
                _FINAL_BLOCK_TEMPLATE.substitute(
                    state=state,
                    content=content,
                    divider=_DIVIDER,
                )
            )

    return "".join(blocks).rstrip("\n")


def _format_state_section(state: str, ctx: Context) -> str:
    """Render entries for *state* in execution order from `ctx.results`."""
    entries = [r for r in ctx.results if r.state == state]
    if not entries:
        return ""

    rows: list[str] = []
    prev_source: str | None = None

    for entry in entries:
        src_label = f"{entry.source:<{_SRC_W}}" if entry.source != prev_source else " " * _SRC_W
        prev_source = entry.source
        if entry.kind == "guard":
            rows.append(f"  {src_label} guard  : {entry.name:<{_NAME_WIDTH}} = {entry.value}")
        else:
            rows.append(f"  {src_label} action : {entry.name:<{_NAME_WIDTH}} = {entry.value!r}")

    return "\n".join(rows) + "\n"
