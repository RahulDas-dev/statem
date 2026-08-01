from __future__ import annotations

import logging
from typing import Any, Final

from pydantic import BaseModel, ConfigDict, Field, PrivateAttr, model_validator

from .schema import (
    ActionFn,
    ActionRegistry,
    ExecutionContext,
    GuardFn,
    GuardRegistry,
    Signal,
    StateConfig,
    TransitionConfig,
    TransitionError,
)

logger = logging.getLogger(__name__)

_ALWAYS_MAX_DEPTH: Final = 100
_ALWAYS_SIGNAL = Signal(event="__always__")  # reused across all runs


class StateMachine(BaseModel):
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
            machine._validate_registries()
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

    def _validate_registries(self) -> None:
        """Validate that all guards and actions referenced in the config are registered."""
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

    # — Public API —————————————————————————————————————————————————————————

    async def run(
        self,
        state_name: str,
        events: Signal | list[Signal],
        session: Any,
        run_id: str | None = None,
    ) -> str:
        """ "Process one or many signals starting from *state*.

        Args:
            state_name: Current state name (e.g. `"idle"`).
            events:     Single `Signal`, list of `Signal`s, or `[]` to
                        only resolve `always` transitions for the current state.
            session:    Caller-owned, opaque payload of any shape (mutated
                        in-place by actions, if they choose to).
            run_id:     Correlation id for this run, used in log lines;
                        auto-generated (`uuid4` hex) when omitted.

        Returns:
            The final state name after all transitions have settled.
        """
        ctx = ExecutionContext(current_state=state_name, session=session, run_id=run_id)
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

    def available_events(self, state_name: str) -> list[str]:
        return list(self.config[state_name].on.keys()) if self.config.get(state_name, None) else []

    # — Internals ——————————————————————————————————————————————————————————

    async def _push_signal(self, ctx: ExecutionContext, signal: Signal) -> bool:
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
            await self._enter(ctx, error_state, signal)
            fired = True

        if fired:
            await self._check_always(ctx)
        return fired

    async def _enter(self, ctx: ExecutionContext, state_name: str, signal: Signal) -> None:
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
            await self.actions.execute_many(exit_actions, ctx, signal, source="exit")
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
            await self.actions.execute_many(entry_actions, ctx, signal, source="entry")

    async def _check_always(self, ctx: ExecutionContext) -> None:
        for _ in range(_ALWAYS_MAX_DEPTH):
            current = ctx.current_state
            always = self.config[current].always
            if not always:
                return
            for t in always:
                if await self.guards.evaluate(t.guard, ctx, _ALWAYS_SIGNAL, source="always"):
                    logger.info(
                        "run_id=%s | state=%s | source=always | name=%s | target=%s",
                        ctx.run_id,
                        current,
                        t.guard,
                        t.target,
                    )
                    await self.actions.execute_many(t.actions, ctx, _ALWAYS_SIGNAL, source="always")
                    await self._enter(ctx, t.target, _ALWAYS_SIGNAL)
                    break
            else:
                return
        raise RuntimeError(f"always-transition loop exceeded {_ALWAYS_MAX_DEPTH} hops at '{ctx.current_state}'")

    async def _try_transitions(
        self, ctx: ExecutionContext, transitions: list[TransitionConfig], signal: Signal
    ) -> bool:
        current = ctx.current_state
        for t in transitions:
            guard_result = await self.guards.evaluate(t.guard, ctx, signal, source="on")
            logger.info(
                "run_id=%s | state=%s | source=guard | name=%s | result=%s",
                ctx.run_id,
                current,
                t.guard or "none",
                guard_result,
            )
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
                    await self.actions.execute_many(t.actions, ctx, signal, source="on")
                except Exception as exc:
                    raise TransitionError(f"Action failed in '{ctx.current_state}'") from exc
                await self._enter(ctx, t.target, signal)
                return True
        return False

    def __repr__(self) -> str:
        states = list(self.config.keys())
        return f"StateMachine(states={states})"
