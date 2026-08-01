from __future__ import annotations

import dataclasses
import unittest

import pydantic

from statem.schema import (
    ActionRegistry,
    Context,
    GuardError,
    GuardRegistry,
    ResultEntry,
    Signal,
    StateConfig,
    TransitionConfig,
)
from tests.helpers import (
    action_returns_value,
    async_action,
    async_guard_false,
    async_guard_true,
    guard_non_bool,
    make_session,
    sync_action,
    sync_guard_false,
    sync_guard_true,
)


class TestSignal(unittest.TestCase):
    def test_default_data(self) -> None:
        signal = Signal(event="START")
        self.assertEqual(signal.event, "START")
        self.assertEqual(signal.data, {})

    def test_custom_data(self) -> None:
        signal = Signal(event="START", data={"key": "value"})
        self.assertEqual(signal.data, {"key": "value"})

    def test_frozen(self) -> None:
        signal = Signal(event="START")
        with self.assertRaises(dataclasses.FrozenInstanceError):
            signal.event = "STOP"  # type: ignore[misc]


class TestContext(unittest.TestCase):
    def test_post_init_seeds_history(self) -> None:
        ctx = Context(current_state="idle", session=make_session())
        self.assertEqual(ctx.history, ["idle"])
        self.assertEqual(ctx.results, [])

    def test_run_id_defaults_to_none_when_omitted(self) -> None:
        ctx = Context(current_state="idle", session=make_session())
        self.assertIsNone(ctx.run_id)

    def test_run_id_explicit_value_preserved(self) -> None:
        ctx = Context(current_state="idle", session=make_session(), run_id="custom-id")
        self.assertEqual(ctx.run_id, "custom-id")

    def test_session_stored_as_is_regardless_of_type(self) -> None:
        sentinel = object()
        ctx = Context(current_state="idle", session=sentinel)
        self.assertIs(ctx.session, sentinel)


class TestActionRegistry(unittest.IsolatedAsyncioTestCase):
    def test_register_has_contains_len(self) -> None:
        registry = ActionRegistry()
        self.assertEqual(len(registry), 0)
        self.assertFalse(registry.has("noop"))
        registry.register("noop", sync_action)
        self.assertTrue(registry.has("noop"))
        self.assertIn("noop", registry)
        self.assertEqual(len(registry), 1)

    def test_register_many(self) -> None:
        registry = ActionRegistry()
        registry.register_many({"noop": sync_action, "aio": async_action})
        self.assertEqual(len(registry), 2)
        self.assertTrue(registry.has("noop"))
        self.assertTrue(registry.has("aio"))

    async def test_execute_sync_fn(self) -> None:
        registry = ActionRegistry()
        registry.register("noop", sync_action)
        ctx = Context(current_state="idle", session=make_session())
        await registry.execute("noop", ctx, Signal(event="X"), source="on")
        self.assertEqual(len(ctx.results), 1)
        self.assertEqual(
            ctx.results[0],
            ResultEntry(state="idle", source="on", kind="action", name="noop", value=None),
        )

    async def test_execute_async_fn(self) -> None:
        registry = ActionRegistry()
        registry.register("aio", async_action)
        ctx = Context(current_state="idle", session=make_session())
        await registry.execute("aio", ctx, Signal(event="X"), source="entry")
        self.assertEqual(ctx.results[0].source, "entry")

    async def test_execute_records_return_value(self) -> None:
        registry = ActionRegistry()
        registry.register("val", action_returns_value)
        ctx = Context(current_state="idle", session=make_session())
        await registry.execute("val", ctx, Signal(event="X"), source="on")
        self.assertEqual(ctx.results[0].value, "action-result")

    async def test_execute_unregistered_raises_keyerror(self) -> None:
        registry = ActionRegistry()
        ctx = Context(current_state="idle", session=make_session())
        with self.assertRaises(KeyError):
            await registry.execute("missing", ctx, Signal(event="X"), source="on")

    async def test_execute_many_preserves_order(self) -> None:
        registry = ActionRegistry()
        registry.register("first", sync_action)
        registry.register("second", async_action)
        ctx = Context(current_state="idle", session=make_session())
        await registry.execute_many(["first", "second"], ctx, Signal(event="X"), source="exit")
        self.assertEqual([r.name for r in ctx.results], ["first", "second"])

    async def test_execute_many_empty_noop(self) -> None:
        registry = ActionRegistry()
        ctx = Context(current_state="idle", session=make_session())
        await registry.execute_many([], ctx, Signal(event="X"), source="on")
        self.assertEqual(ctx.results, [])


class TestGuardRegistry(unittest.IsolatedAsyncioTestCase):
    def test_register_has_contains_len(self) -> None:
        registry = GuardRegistry()
        self.assertEqual(len(registry), 0)
        registry.register("always_true", sync_guard_true)
        self.assertTrue(registry.has("always_true"))
        self.assertIn("always_true", registry)
        self.assertEqual(len(registry), 1)

    def test_register_many(self) -> None:
        registry = GuardRegistry()
        registry.register_many({"a": sync_guard_true, "b": async_guard_true})
        self.assertEqual(len(registry), 2)

    async def test_evaluate_none_guard_returns_true_without_result_entry(self) -> None:
        registry = GuardRegistry()
        ctx = Context(current_state="idle", session=make_session())
        result = await registry.evaluate(None, ctx, Signal(event="X"), source="on")
        self.assertTrue(result)
        self.assertEqual(ctx.results, [])

    async def test_evaluate_unregistered_raises_keyerror(self) -> None:
        registry = GuardRegistry()
        ctx = Context(current_state="idle", session=make_session())
        with self.assertRaises(KeyError):
            await registry.evaluate("missing", ctx, Signal(event="X"), source="on")

    async def test_evaluate_sync_true(self) -> None:
        registry = GuardRegistry()
        registry.register("g", sync_guard_true)
        ctx = Context(current_state="idle", session=make_session())
        result = await registry.evaluate("g", ctx, Signal(event="X"), source="on")
        self.assertTrue(result)
        self.assertEqual(ctx.results[0], ResultEntry(state="idle", source="on", kind="guard", name="g", value=True))

    async def test_evaluate_sync_false(self) -> None:
        registry = GuardRegistry()
        registry.register("g", sync_guard_false)
        ctx = Context(current_state="idle", session=make_session())
        result = await registry.evaluate("g", ctx, Signal(event="X"), source="on")
        self.assertFalse(result)

    async def test_evaluate_async_true(self) -> None:
        registry = GuardRegistry()
        registry.register("g", async_guard_true)
        ctx = Context(current_state="idle", session=make_session())
        result = await registry.evaluate("g", ctx, Signal(event="X"), source="always")
        self.assertTrue(result)

    async def test_evaluate_async_false(self) -> None:
        registry = GuardRegistry()
        registry.register("g", async_guard_false)
        ctx = Context(current_state="idle", session=make_session())
        result = await registry.evaluate("g", ctx, Signal(event="X"), source="always")
        self.assertFalse(result)

    async def test_evaluate_non_bool_raises_guarderror(self) -> None:
        registry = GuardRegistry()
        registry.register("g", guard_non_bool)
        ctx = Context(current_state="idle", session=make_session())
        with self.assertRaises(GuardError):
            await registry.evaluate("g", ctx, Signal(event="X"), source="on")


class TestTransitionConfig(unittest.TestCase):
    def test_defaults(self) -> None:
        tc = TransitionConfig(target="running")
        self.assertEqual(tc.target, "running")
        self.assertIsNone(tc.guard)
        self.assertEqual(tc.actions, [])

    def test_frozen_raises(self) -> None:
        tc = TransitionConfig(target="running")
        with self.assertRaises(pydantic.ValidationError):
            tc.target = "idle"


class TestStateConfigNormalization(unittest.TestCase):
    def test_on_string_shorthand(self) -> None:
        cfg = StateConfig.model_validate({"on": {"START": "running"}})
        self.assertEqual(cfg.on["START"], [TransitionConfig(target="running")])

    def test_on_dict_shorthand(self) -> None:
        cfg = StateConfig.model_validate({"on": {"START": {"target": "running", "guard": "can_start"}}})
        self.assertEqual(cfg.on["START"], [TransitionConfig(target="running", guard="can_start")])

    def test_on_list_mixed(self) -> None:
        cfg = StateConfig.model_validate({"on": {"START": ["running", {"target": "error", "guard": "failed"}]}})
        self.assertEqual(
            cfg.on["START"],
            [TransitionConfig(target="running"), TransitionConfig(target="error", guard="failed")],
        )

    def test_on_not_dict_raises_validation_error(self) -> None:
        with self.assertRaises(pydantic.ValidationError):
            StateConfig.model_validate({"on": None})

    def test_on_event_spec_unrecognized_type_raises_validation_error(self) -> None:
        with self.assertRaises(pydantic.ValidationError):
            StateConfig.model_validate({"on": {"START": None}})

    def test_always_string_shorthand(self) -> None:
        cfg = StateConfig.model_validate({"always": ["done"]})
        self.assertEqual(cfg.always, [TransitionConfig(target="done")])

    def test_always_not_list_raises_validation_error(self) -> None:
        with self.assertRaises(pydantic.ValidationError):
            StateConfig.model_validate({"always": "done"})

    def test_extra_fields_ignored(self) -> None:
        cfg = StateConfig.model_validate({"role": "user", "render": "screen", "intent": "collect"})
        self.assertFalse(hasattr(cfg, "role"))

    def test_available_events_excludes_wildcard(self) -> None:
        cfg = StateConfig.model_validate({"on": {"START": "running", "CANCEL": "idle", "*": "error"}})
        self.assertEqual(cfg.available_events, ["START", "CANCEL"])

    def test_accepts_wildcard_true(self) -> None:
        cfg = StateConfig.model_validate({"on": {"*": "error"}})
        self.assertTrue(cfg.accepts_wildcard)

    def test_accepts_wildcard_false(self) -> None:
        cfg = StateConfig.model_validate({"on": {"START": "running"}})
        self.assertFalse(cfg.accepts_wildcard)


if __name__ == "__main__":
    unittest.main()
