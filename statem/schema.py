import inspect
import logging
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from functools import cached_property
from typing import Any, Generic, Literal, NamedTuple, TypeAlias, TypeVar

from pydantic import BaseModel, ConfigDict, Field, field_validator

T = TypeVar("T")

logger = logging.getLogger(__name__)


class GuardError(TypeError):
    """Raised when a guard function returns a non-bool value."""


class TransitionError(Exception):
    """Raised when an action fails during a transition."""


# — Runtime primitives ———————————————————————————————————————————————————


@dataclass(frozen=True, slots=True)
class Signal:
    """A signal that triggers a transition.

    Attributes:
        event: Event name (upper-case by convention, e.g. `"START"`).
        data: Arbitrary payload dict; defaults to empty.
    """

    event: str
    data: dict[str, Any] = field(default_factory=dict)


HookSource: TypeAlias = Literal["on", "always", "entry", "exit"]


class ResultEntry(NamedTuple):
    """A single recorded guard or action result, in execution order.

    Attributes:
        state: Name of the state the machine was in when this fired.
        source: Lifecycle hook: `"on"`, `"always"`, `"entry"`, or `"exit"`.
        kind: `"guard"` or `"action"`.
        name: Registered name of the guard or action function.
        value: `bool` for guards; return value (may be `None`) for actions.
    """

    state: str
    source: HookSource
    kind: Literal["guard", "action"]
    name: str
    value: Any


@dataclass(slots=True, kw_only=True)
class Context(Generic[T]):
    """Short-lived execution context passed to every action and guard during one `run()` call.

    Created at the start of `StateMachine.run` and discarded when it returns. The machine updates
    `current_state` as transitions fire. All fields are keyword-only.

    Generic over `T`, the type of `session`. `Context` used bare (unparameterized) behaves exactly
    as before -- `session` types as `Any`. Guards/actions that want typed access to their own
    session shape can annotate their `ctx` parameter as `Context[BankSession]` (or whatever their
    session type is) to get full type-checking and autocomplete on `ctx.session`.

    Attributes:
        run_id: Correlation id for this `run()` call, used in log lines. `None` if the caller
            didn't supply one -- `StateMachine.run()` is responsible for generating one (a
            `uuid4().hex` string) before constructing this object; `Context` itself just stores
            whatever it's given.
        current_state: Name of the state the machine is currently in; updated by the engine as
            each transition fires.
        session: Caller-owned, opaque payload of any shape; the engine never inspects it, only
            threads it through to actions/guards.
        history: Ordered list of every state entered during this `run()` call; the first entry
            is always the initial state.
        results: Ordered list of `ResultEntry` for every guard and action executed, in the exact
            order they fired during this `run()` call.
    """

    run_id: str | None = None
    current_state: str
    session: T
    history: list[str] = field(init=False)
    results: list[ResultEntry] = field(init=False, default_factory=list)

    def __post_init__(self) -> None:
        self.history = [self.current_state]


# — Callable type aliases ————————————————————————————————————————————————

ActionFn: TypeAlias = Callable[[Context, Signal], Awaitable[Any] | Any]

GuardFn: TypeAlias = Callable[[Context, Signal], bool] | Callable[[Context, Signal], Awaitable[bool]]


# — Registries ——————————————————————————————————————————————————————————


class _RegEntry(NamedTuple):
    """Internal registry entry: pairs a callable with its async flag, cached at registration time."""

    fn: Callable[..., Any]
    is_async: bool


class ActionRegistry:
    """Registry of named action functions (sync or async)."""

    __slots__ = ("_fns",)

    def __init__(self) -> None:
        self._fns: dict[str, _RegEntry] = {}

    def register(self, name: str, fn: ActionFn) -> None:
        """Register a single named action function (sync or async)."""
        self._fns[name] = _RegEntry(fn=fn, is_async=inspect.iscoroutinefunction(fn))

    def register_many(self, actions: dict[str, ActionFn]) -> None:
        """Register multiple named action functions at once."""
        for name, fn in actions.items():
            self.register(name, fn)

    def has(self, name: str) -> bool:
        """Return `True` if an action named `name` is registered."""
        return name in self._fns

    def __contains__(self, name: str) -> bool:
        return name in self._fns

    def __len__(self) -> int:
        return len(self._fns)

    async def execute(self, name: str, ctx: Context, signal: Signal, source: HookSource) -> None:
        """Execute a single named action. Raises `KeyError` if not registered."""
        if name not in self._fns:
            raise KeyError(f"'{name}' not registered")
        entry = self._fns[name]
        value = await entry.fn(ctx, signal) if entry.is_async else entry.fn(ctx, signal)
        ctx.results.append(ResultEntry(state=ctx.current_state, source=source, kind="action", name=name, value=value))

    async def execute_many(
        self,
        names: list[str],
        ctx: Context,
        signal: Signal,
        source: HookSource,
    ) -> None:
        """Execute actions in order, awaiting each one."""
        for name in names:
            await self.execute(name, ctx, signal, source)


class GuardRegistry:
    """Registry of named guard functions (sync or async).

    Async detection is cached at register time.
    """

    __slots__ = ("_fns",)

    def __init__(self) -> None:
        self._fns: dict[str, _RegEntry] = {}

    def register(self, name: str, fn: GuardFn) -> None:
        """Register a single named guard function (sync or async)."""
        self._fns[name] = _RegEntry(fn=fn, is_async=inspect.iscoroutinefunction(fn))

    def register_many(self, guards: dict[str, GuardFn]) -> None:
        """Register multiple named guard functions at once."""
        for name, fn in guards.items():
            self.register(name, fn)

    def has(self, name: str) -> bool:
        """Return `True` if a guard named `name` is registered."""
        return name in self._fns

    def __contains__(self, name: str) -> bool:
        return name in self._fns

    def __len__(self) -> int:
        return len(self._fns)

    async def evaluate(self, name: str | None, ctx: Context, signal: Signal, source: HookSource) -> bool:
        """Evaluate a guard. Returns `True` if `name` is `None` (no guard = pass)."""
        if name is None:
            return True
        if name not in self._fns:
            raise KeyError(f"'{name}' not registered")
        entry = self._fns[name]
        raw = await entry.fn(ctx, signal) if entry.is_async else entry.fn(ctx, signal)
        if not isinstance(raw, bool):
            raise GuardError(f"Guard '{name}' must return bool, got {type(raw).__name__!r}")
        ctx.results.append(ResultEntry(state=ctx.current_state, source=source, kind="guard", name=name, value=raw))
        logger.debug("Guard '%s' -> %s", name, raw)
        return raw


# — Config models ————————————————————————————————————————————————————————


class TransitionConfig(BaseModel):
    """Configuration for a single transition candidate.

    Attributes:
        target: Name of the state to transition to.
        guard: Registered guard name that must pass for this transition to fire; `None` means
            the transition always passes.
        actions: Ordered list of registered action names to execute when this transition fires.
    """

    model_config = ConfigDict(frozen=True)

    target: str
    guard: str | None = Field(default=None)
    actions: list[str] = Field(default_factory=list)


class StateConfig(BaseModel):
    """Configuration for one state.

    Extra fields (e.g. `role`, `intent`, `render`) are accepted and silently ignored, so raw
    application config dicts can be passed directly. Each `on` value is normalized to a list of
    `TransitionConfig` at parse time.

    Attributes:
        on: Maps event names to transition candidates.
        always: Eventless transitions checked after every state entry.
        entry: Action names to fire when entering this state.
        exit: Action names to fire when leaving this state.
        error_state: Fallback state on unhandled action errors.
    """

    model_config = ConfigDict(frozen=True, extra="ignore")

    on: dict[str, list[TransitionConfig]] = Field(default_factory=dict)
    always: list[TransitionConfig] = Field(default_factory=list)
    entry: list[str] = Field(default_factory=list)
    exit: list[str] = Field(default_factory=list)
    error_state: str | None = None

    @field_validator("on", mode="before")
    @classmethod
    def _normalize_on(cls, v: Any) -> Any:
        """Normalize each event value to a list of TransitionConfig-compatible dicts."""
        if not isinstance(v, dict):
            return v
        result: dict[str, list[Any]] = {}
        for event, spec in v.items():
            if isinstance(spec, str):
                result[event] = [{"target": spec}]
            elif isinstance(spec, dict):
                result[event] = [spec]
            elif isinstance(spec, list):
                result[event] = [{"target": item} if isinstance(item, str) else item for item in spec]
            else:
                result[event] = spec
        return result

    @field_validator("always", mode="before")
    @classmethod
    def _normalize_always(cls, v: Any) -> Any:
        """Normalize each always item to a TransitionConfig-compatible dict."""
        if not isinstance(v, list):
            return v
        return [{"target": item} if isinstance(item, str) else item for item in v]

    @cached_property
    def available_events(self) -> list[str]:
        """Return the event names this state can receive, in declaration order.

        Excludes the `"*"` wildcard entry -- use `accepts_wildcard` to check whether the state
        has a catch-all handler.

        Example:
            ```python
            cfg = StateConfig.model_validate({
                "on": {"START": "running", "CANCEL": "idle", "*": "error"}
            })
            cfg.available_events  # ["START", "CANCEL"]
            cfg.accepts_wildcard  # True
            ```
        """
        return [name for name in self.on if name != "*"]

    @property
    def accepts_wildcard(self) -> bool:
        """Return `True` if this state has a `"*"` catch-all transition."""
        return "*" in self.on
