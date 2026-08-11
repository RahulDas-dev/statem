from __future__ import annotations

import asyncio
import logging
import uuid
from typing import TYPE_CHECKING, Any, Final

from pydantic import BaseModel, ConfigDict, Field, PrivateAttr, model_validator

from .schema import (
    ActionFn,
    ActionRegistry,
    Context,
    GuardFn,
    GuardRegistry,
    ResultEntry,
    Signal,
    StateConfig,
    TransitionConfig,
    TransitionError,
)

if TYPE_CHECKING:
    from collections.abc import AsyncIterator, Callable

    from ag_ui.core import BaseEvent

logger = logging.getLogger(__name__)

_ALWAYS_MAX_DEPTH: Final = 100
_ALWAYS_SIGNAL = Signal(event="__always__")  # reused across all runs


class StateMachine(BaseModel):
    """A validated, immutable state graph paired with registries of named guards/actions.

    Build one with `from_dict`, then drive it with `await run(...)`. Instances are frozen and
    hold no per-run state, so a single `StateMachine` can safely process many concurrent runs --
    all mutable state lives in the `Context` created fresh for each `run()` call.

    Attributes:
        config: State name to `StateConfig` mapping -- the validated transition graph.
        guards: Registry of named guard functions, evaluated to decide which transition fires.
        actions: Registry of named action functions, executed on entry/exit/transition.
    """

    model_config = ConfigDict(frozen=True, arbitrary_types_allowed=True)

    config: dict[str, StateConfig] = Field(default_factory=dict)
    guards: GuardRegistry = Field(default_factory=GuardRegistry)
    actions: ActionRegistry = Field(default_factory=ActionRegistry)

    _transition_table: dict[tuple[str, str], list[TransitionConfig]] = PrivateAttr(default_factory=dict)

    @classmethod
    def from_dict(
        cls,
        config: dict[str, Any],
        action_dict: dict[str, ActionFn] | None = None,
        guard_dict: dict[str, GuardFn] | None = None,
    ) -> StateMachine:
        """Construct, register actions/guards, then validate.

        `validate_registries()` is called automatically when at least one of
        *action_dict* / *guard_dict* is supplied. If you register later via
        the registries directly, call `validate_registries()` yourself.
        """
        states = {k: StateConfig.model_validate(v) for k, v in config.items()}
        machine = cls(config=states)
        if action_dict:
            machine.actions.register_many(action_dict)
        if guard_dict:
            machine.guards.register_many(guard_dict)
        if action_dict or guard_dict:
            machine.validate_registries()
        return machine

    @model_validator(mode="after")
    def _validate_machine(self) -> StateMachine:
        """Validate transition targets and build the transition lookup table."""
        bad_targets: list[str] = []

        for state_id, state_cfg in self.config.items():
            bad_targets.extend(
                f"{state_id}.on.{sig_name} -> '{t.target}'"
                for sig_name, candidates in state_cfg.on.items()
                for t in candidates
                if t.target not in self.config
            )

            for idx, t in enumerate(state_cfg.always):
                if t.target not in self.config:
                    bad_targets.append(f"{state_id}.always[{idx}] -> '{t.target}'")

            if state_cfg.error_state and state_cfg.error_state not in self.config:
                bad_targets.append(f"{state_id}.error_state -> '{state_cfg.error_state}'")

        if bad_targets:
            raise ValueError(f"Invalid transition targets: {bad_targets}")

        # Transition table: (state_id, signal_type) -> candidates
        table: dict[tuple[str, str], list[TransitionConfig]] = {}
        for sid, scfg in self.config.items():
            for sname, cands in scfg.on.items():
                table[(sid, sname)] = cands
        self._transition_table = table

        return self

    # — Public API —————————————————————————————————————————————————————————

    def validate_registries(self) -> None:
        """Validate that all guards and actions referenced in `config` are registered.

        Called automatically by `from_dict` when at least one of *action_dict* / *guard_dict* is
        supplied. If you register actions/guards later via `self.actions`/`self.guards` directly,
        call this yourself to check for typos -- raises `ValueError` listing anything missing.
        """
        missing_guards: list[str] = []
        missing_actions: list[str] = []

        for state_id, state_cfg in self.config.items():
            missing_actions.extend(f"{state_id}.entry: {n}" for n in state_cfg.entry if not self.actions.has(n))
            missing_actions.extend(f"{state_id}.exit: {n}" for n in state_cfg.exit if not self.actions.has(n))

            for sig_name, candidates in state_cfg.on.items():
                for t in candidates:
                    if t.guard and not self.guards.has(t.guard):
                        missing_guards.append(f"{state_id}.on.{sig_name}: {t.guard}")
                    missing_actions.extend(
                        f"{state_id}.on.{sig_name}: {n}" for n in t.actions if not self.actions.has(n)
                    )

            for idx, t in enumerate(state_cfg.always):
                if t.guard and not self.guards.has(t.guard):
                    missing_guards.append(f"{state_id}.always[{idx}]: {t.guard}")
                missing_actions.extend(f"{state_id}.always[{idx}]: {n}" for n in t.actions if not self.actions.has(n))

        errors: list[str] = []
        if missing_guards:
            errors.append(f"Unregistered guards: {missing_guards}")
        if missing_actions:
            errors.append(f"Unregistered actions: {missing_actions}")
        if errors:
            raise ValueError("; ".join(errors))

    async def run(
        self,
        *,
        run_id: str | None = None,
        thread_id: str | None = None,
        state_name: str,
        events: Signal | list[Signal],
        session: Any,
    ) -> str:
        """Process one or many signals starting from *state*. All arguments are keyword-only.

        Args:
            run_id:     Correlation id for this run, used in log lines. If not provided (left as
                        `None`), a `uuid4().hex` string is generated automatically.
            thread_id:  Correlation id for the broader conversation/session this run belongs to
                        (one thread can span many runs). If not provided, a `uuid4().hex` string
                        is generated automatically, same as `run_id`.
            state_name: Current state name (e.g. `"idle"`).
            events:     Single `Signal`, list of `Signal`s, or `[]` to
                        only resolve `always` transitions for the current state.
            session:    Caller-owned, opaque payload of any shape (mutated
                        in-place by actions, if they choose to).

        Returns:
            The final state name after all transitions have settled.
        """
        if run_id is None:
            run_id = uuid.uuid4().hex
        if thread_id is None:
            thread_id = uuid.uuid4().hex
        ctx = Context(run_id=run_id, thread_id=thread_id, current_state=state_name, session=session)
        await self._check_always(ctx)
        items = (events,) if isinstance(events, Signal) else events
        for signal in items:
            logger.info(
                "run_id=%s | state=%s | source=signal | event=%s",
                ctx.run_id,
                ctx.current_state,
                signal.event,
            )
            await self._push_signal(ctx, signal)
        return ctx.current_state

    async def stream(  # noqa: PLR0913 -- all keyword-only, mirrors `run`'s parameter set plus `state_accessor`
        self,
        *,
        run_id: str | None = None,
        thread_id: str | None = None,
        state_name: str,
        events: Signal | list[Signal],
        session: Any,
        state_accessor: Callable[[Any], dict[str, Any]] | None = None,
    ) -> AsyncIterator[BaseEvent]:
        """Like `run`, but yields AG-UI protocol events as the machine executes.

        Drives the same engine as `run` (same guards, actions, transition rules) and has the
        same effect on *session* -- this is an additive, alternate way to observe a run, not a
        different execution path. Requires the `agui` extra: `pip install statem[agui]`.

        A step = one state change (one hop), whether triggered by an `on` transition, an
        `always` cascade hop, or an `error_state` fallback -- not one call to `stream()`. Each
        step is fully self-contained, emitted in this order:

        - `STEP_STARTED` (`step_name` is the triggering signal's event, or `"__always__"` for an
          `always`-cascade hop).
        - `STATE_SNAPSHOT`, taken right before this hop's actions run.
        - `ACTIVITY_SNAPSHOT` for every guard and action result as it fires during this hop
          (`content` carries `name`, hook `source`, and `result`). A guard evaluated but not
          taken (e.g. an earlier candidate that failed) is reported the same way, just before its
          step -- or the step before it -- opens.
        - `STATE_DELTA` (RFC 6902 patch, via `jsonpatch.make_patch`, against this step's own
          `STATE_SNAPSHOT`) -- skipped if this step didn't actually change the broadcast state.
        - `STEP_FINISHED`.

        A signal that matches no transition, or whose guard(s) all fail, produces no events at
        all (no state change happened). `RUN_STARTED` / `RUN_FINISHED` / `RUN_ERROR` are never
        emitted. An unhandled exception (e.g. an action error with no `error_state`, or a bad
        guard return type) propagates to the caller from the generator itself, exactly as it
        would from `run`.

        Args:
            run_id:         Correlation id for this run. Auto-generated if not provided.
            thread_id:      Correlation id for the broader conversation/session this run belongs
                            to (one thread can span many runs). Auto-generated if not provided,
                            same as `run_id`.
            state_name:     Current state name (e.g. `"idle"`).
            events:         Single `Signal`, list of `Signal`s, or `[]` to only resolve `always`
                            transitions for the current state.
            session:        Caller-owned, opaque payload of any shape.
            state_accessor: Derives the dict broadcast via `STATE_SNAPSHOT`/`STATE_DELTA` from
                            *session* (e.g. `lambda session: session.to_dict()`). Called at the
                            start and end of every step. Defaults to `{"current_state": <state
                            name>}` when omitted.

        Yields:
            `ag_ui.core.BaseEvent` instances in execution order.
        """
        if run_id is None:
            run_id = uuid.uuid4().hex
        if thread_id is None:
            thread_id = uuid.uuid4().hex

        queue: asyncio.Queue[BaseEvent | None] = asyncio.Queue()
        ctx = Context(
            run_id=run_id,
            thread_id=thread_id,
            current_state=state_name,
            session=session,
            _event_q=queue,
            _state_accessor=state_accessor,
        )

        async def _drive() -> None:
            try:
                await self._check_always(ctx)
                items = (events,) if isinstance(events, Signal) else events
                for signal in items:
                    logger.info(
                        "run_id=%s | state=%s | source=signal | event=%s",
                        ctx.run_id,
                        ctx.current_state,
                        signal.event,
                    )
                    await self._push_signal(ctx, signal)
            finally:
                queue.put_nowait(None)

        task = asyncio.ensure_future(_drive())
        try:
            while True:
                item = await queue.get()
                if item is None:
                    break
                yield item
        finally:
            await task

    def available_events(self, state_name: str) -> list[str]:
        """Return the event names `state_name` can receive via `on`, or `[]` if the state is unknown.

        Excludes the `"*"` wildcard entry -- use `StateConfig.accepts_wildcard` to check for a
        catch-all handler.
        """
        return list(self.config[state_name].on.keys()) if self.config.get(state_name, None) else []

    # — Internals ——————————————————————————————————————————————————————————

    def _current_state_dict(self, ctx: Context) -> dict[str, Any]:
        if ctx._state_accessor:  # noqa: SLF001
            return ctx._state_accessor(ctx.session)  # noqa: SLF001
        return {"current_state": ctx.current_state}

    def _open_step(self, ctx: Context, step_name: str) -> None:
        """Open one AG-UI step: `STEP_STARTED` followed immediately by a `STATE_SNAPSHOT`.

        Resets `ctx._state_snapshot` to the state *right now* (before this hop's guard/actions
        run) so `_close_step` can diff against it -- each step's `STATE_DELTA` reflects only what
        changed during that step, not the whole stream.
        """
        if not ctx.is_stream:
            return
        from . import agui  # noqa: PLC0415 -- lazy so plain `import statem` never needs ag-ui-protocol

        ctx._event_q.put_nowait(agui.step_started(step_name))  # noqa: SLF001
        snapshot = self._current_state_dict(ctx)
        ctx._state_snapshot = snapshot  # noqa: SLF001
        ctx._event_q.put_nowait(agui.state_snapshot(snapshot))  # noqa: SLF001

    def _close_step(self, ctx: Context, step_name: str) -> None:
        """Close one AG-UI step: `STATE_DELTA` (if anything changed) then `STEP_FINISHED`."""
        if not ctx.is_stream:
            return
        from . import agui  # noqa: PLC0415 -- lazy so plain `import statem` never needs ag-ui-protocol

        new_state = self._current_state_dict(ctx)
        patch = agui.diff_state(ctx._state_snapshot or {}, new_state)  # noqa: SLF001
        ctx._state_snapshot = new_state  # noqa: SLF001
        if patch:
            ctx._event_q.put_nowait(agui.state_delta(patch))  # noqa: SLF001
        ctx._event_q.put_nowait(agui.step_finished(step_name))  # noqa: SLF001

    def _emit_activities(self, ctx: Context, entries: list[ResultEntry]) -> None:
        if not ctx.is_stream or not entries:
            return
        from . import agui  # noqa: PLC0415 -- lazy so plain `import statem` never needs ag-ui-protocol

        for entry in entries:
            ctx._event_q.put_nowait(agui.activity(entry))  # noqa: SLF001

    async def _push_signal(self, ctx: Context, signal: Signal) -> bool:
        current = ctx.current_state
        transitions = self._transition_table.get((current, signal.event)) or self._transition_table.get((current, "*"))
        if not transitions:
            return False

        try:
            fired = await self._try_transitions(ctx, transitions, signal)
        except TransitionError:
            error_state = self.config[current].error_state
            if error_state is None:
                raise
            logger.warning(
                "run_id=%s | state=%s | source=error | target=%s",
                ctx.run_id,
                current,
                error_state,
            )
            self._open_step(ctx, signal.event)
            await self._enter(ctx, error_state, signal)
            self._close_step(ctx, signal.event)
            fired = True

        if fired:
            await self._check_always(ctx)
        return fired

    async def _enter(self, ctx: Context, state_name: str, signal: Signal) -> None:
        current = ctx.current_state
        exit_actions = self.config[current].exit
        if exit_actions:
            for name in exit_actions:
                logger.info(
                    "run_id=%s | state=%s | source=exit | name=%s",
                    ctx.run_id,
                    current,
                    name,
                )
            n_before = len(ctx.results)
            await self.actions.execute_many(exit_actions, ctx, signal, source="exit")
            self._emit_activities(ctx, ctx.results[n_before:])
        logger.info(
            "run_id=%s | state=%s | source=enter | target=%s",
            ctx.run_id,
            current,
            state_name,
        )
        ctx.history.append(state_name)
        ctx.current_state = state_name
        entry_actions = self.config[state_name].entry
        if entry_actions:
            for name in entry_actions:
                logger.info(
                    "run_id=%s | state=%s | source=entry | name=%s",
                    ctx.run_id,
                    state_name,
                    name,
                )
            n_before = len(ctx.results)
            await self.actions.execute_many(entry_actions, ctx, signal, source="entry")
            self._emit_activities(ctx, ctx.results[n_before:])

    async def _check_always(self, ctx: Context) -> None:
        for _ in range(_ALWAYS_MAX_DEPTH):
            current = ctx.current_state
            always = self.config[current].always
            if not always:
                return
            self._open_step(ctx, _ALWAYS_SIGNAL.event)
            fired = False
            for t in always:
                guard_result = await self.guards.evaluate(t.guard, ctx, _ALWAYS_SIGNAL, source="always")
                if t.guard is not None:
                    self._emit_activities(ctx, [ctx.results[-1]])
                if guard_result:
                    logger.info(
                        "run_id=%s | state=%s | source=always | name=%s | target=%s",
                        ctx.run_id,
                        current,
                        t.guard,
                        t.target,
                    )
                    n_before = len(ctx.results)
                    await self.actions.execute_many(t.actions, ctx, _ALWAYS_SIGNAL, source="always")
                    self._emit_activities(ctx, ctx.results[n_before:])
                    await self._enter(ctx, t.target, _ALWAYS_SIGNAL)
                    fired = True
                    break
            self._close_step(ctx, _ALWAYS_SIGNAL.event)
            if not fired:
                return
        raise RuntimeError(f"always-transition loop exceeded {_ALWAYS_MAX_DEPTH} hops at '{ctx.current_state}'")

    async def _try_transitions(self, ctx: Context, transitions: list[TransitionConfig], signal: Signal) -> bool:
        current = ctx.current_state
        self._open_step(ctx, signal.event)
        try:
            for t in transitions:
                guard_result = await self.guards.evaluate(t.guard, ctx, signal, source="on")
                logger.info(
                    "run_id=%s | state=%s | source=guard | name=%s | result=%s",
                    ctx.run_id,
                    current,
                    t.guard or "none",
                    guard_result,
                )
                if t.guard is not None:
                    self._emit_activities(ctx, [ctx.results[-1]])
                if guard_result:
                    logger.info(
                        "run_id=%s | state=%s | source=transition | event=%s | target=%s | actions=%s",
                        ctx.run_id,
                        current,
                        signal.event,
                        t.target,
                        t.actions,
                    )
                    try:
                        n_before = len(ctx.results)
                        await self.actions.execute_many(t.actions, ctx, signal, source="on")
                        self._emit_activities(ctx, ctx.results[n_before:])
                    except Exception as exc:
                        raise TransitionError(f"Action failed in '{ctx.current_state}'") from exc
                    await self._enter(ctx, t.target, signal)
                    return True
            return False
        finally:
            self._close_step(ctx, signal.event)

    def __repr__(self) -> str:
        states = list(self.config.keys())
        return f"StateMachine(states={states})"
