from __future__ import annotations

import unittest

import pydantic

from statem import (
    ExecutionContext,
    Signal,
    StateConfig,
    StateMachine,
    TransitionConfig,
    TransitionError,
)
from tests.helpers import (
    action_raises,
    make_session,
    sync_action,
    sync_guard_false,
    sync_guard_true,
)


class TestFromDict(unittest.TestCase):
    def test_without_dicts_skips_validation(self) -> None:
        cfg = {
            "idle": {"on": {"START": {"target": "running", "guard": "ghost_guard", "actions": ["ghost_action"]}}},
            "running": {},
        }
        machine = StateMachine.from_dict(cfg)
        self.assertFalse(machine.guards.has("ghost_guard"))
        self.assertFalse(machine.actions.has("ghost_action"))

    def test_missing_guard_raises(self) -> None:
        cfg = {"idle": {"on": {"START": {"target": "running", "guard": "ghost_guard"}}}, "running": {}}
        with self.assertRaises(ValueError) as cm:
            StateMachine.from_dict(cfg, guard_dict={"other": sync_guard_true})
        self.assertIn("ghost_guard", str(cm.exception))

    def test_missing_entry_action_raises(self) -> None:
        cfg = {"idle": {"entry": ["ghost_entry"]}}
        with self.assertRaises(ValueError) as cm:
            StateMachine.from_dict(cfg, action_dict={"other": sync_action})
        self.assertIn("idle.entry: ghost_entry", str(cm.exception))

    def test_missing_exit_action_raises(self) -> None:
        cfg = {"idle": {"exit": ["ghost_exit"]}}
        with self.assertRaises(ValueError) as cm:
            StateMachine.from_dict(cfg, action_dict={"other": sync_action})
        self.assertIn("idle.exit: ghost_exit", str(cm.exception))

    def test_missing_on_action_raises(self) -> None:
        cfg = {"idle": {"on": {"START": {"target": "running", "actions": ["ghost_on_action"]}}}, "running": {}}
        with self.assertRaises(ValueError) as cm:
            StateMachine.from_dict(cfg, action_dict={"other": sync_action})
        self.assertIn("idle.on.START: ghost_on_action", str(cm.exception))

    def test_missing_always_guard_and_action_raise(self) -> None:
        cfg = {"idle": {"always": [{"target": "idle", "guard": "ghost_guard", "actions": ["ghost_action"]}]}}
        with self.assertRaises(ValueError) as cm:
            StateMachine.from_dict(
                cfg,
                action_dict={"other": sync_action},
                guard_dict={"other": sync_guard_true},
            )
        message = str(cm.exception)
        self.assertIn("idle.always[0]: ghost_guard", message)
        self.assertIn("idle.always[0]: ghost_action", message)


class TestValidateMachine(unittest.TestCase):
    def test_bad_on_target_raises(self) -> None:
        cfg = {"idle": StateConfig.model_validate({"on": {"START": "ghost"}})}
        with self.assertRaises(pydantic.ValidationError) as cm:
            StateMachine(config=cfg)
        self.assertIn("idle.on.START -> 'ghost'", str(cm.exception))

    def test_bad_always_target_raises(self) -> None:
        cfg = {"idle": StateConfig.model_validate({"always": ["ghost"]})}
        with self.assertRaises(pydantic.ValidationError) as cm:
            StateMachine(config=cfg)
        self.assertIn("idle.always[0] -> 'ghost'", str(cm.exception))

    def test_bad_error_state_target_raises(self) -> None:
        cfg = {"idle": StateConfig.model_validate({"error_state": "ghost"})}
        with self.assertRaises(pydantic.ValidationError) as cm:
            StateMachine(config=cfg)
        self.assertIn("idle.error_state -> 'ghost'", str(cm.exception))

    def test_valid_config_builds_transition_table(self) -> None:
        cfg = {
            "idle": StateConfig.model_validate({"on": {"START": "running"}}),
            "running": StateConfig.model_validate({}),
        }
        machine = StateMachine(config=cfg)
        self.assertEqual(machine._transition_table[("idle", "START")], [TransitionConfig(target="running")])


class TestRun(unittest.IsolatedAsyncioTestCase):
    async def test_single_signal_transitions(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        result = await machine.run("idle", Signal(event="START"), make_session())
        self.assertEqual(result, "running")

    async def test_list_of_signals_processed_sequentially(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {"on": {"STOP": "idle"}}}
        machine = StateMachine.from_dict(cfg)
        result = await machine.run("idle", [Signal(event="START"), Signal(event="STOP")], make_session())
        self.assertEqual(result, "idle")

    async def test_empty_events_only_resolves_always(self) -> None:
        cfg = {
            "idle": {"always": [{"target": "running", "guard": "always_true"}]},
            "running": {},
        }
        machine = StateMachine.from_dict(cfg, guard_dict={"always_true": sync_guard_true})
        result = await machine.run("idle", [], make_session())
        self.assertEqual(result, "running")

    async def test_unknown_event_state_unchanged(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        result = await machine.run("idle", Signal(event="UNKNOWN"), make_session())
        self.assertEqual(result, "idle")

    async def test_wildcard_fallback(self) -> None:
        cfg = {"idle": {"on": {"*": "fallback"}}, "fallback": {}}
        machine = StateMachine.from_dict(cfg)
        result = await machine.run("idle", Signal(event="ANYTHING"), make_session())
        self.assertEqual(result, "fallback")

    async def test_run_forwards_explicit_run_id_to_logs(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        with self.assertLogs("statem.machine", level="INFO") as cm:
            await machine.run("idle", Signal(event="START"), make_session(), run_id="my-correlation-id")
        self.assertTrue(any("run_id=my-correlation-id" in line for line in cm.output))

    async def test_run_auto_generates_run_id_when_omitted(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        with self.assertLogs("statem.machine", level="INFO") as cm:
            await machine.run("idle", Signal(event="START"), make_session())
        self.assertTrue(any("run_id=" in line and "run_id=None" not in line for line in cm.output))

    async def test_run_accepts_arbitrary_session_type(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        result = await machine.run("idle", Signal(event="START"), "anything-goes")
        self.assertEqual(result, "running")


class TestPushSignal(unittest.IsolatedAsyncioTestCase):
    async def test_no_guard_passes_returns_false(self) -> None:
        cfg = {"idle": {"on": {"START": {"target": "running", "guard": "guard_false"}}}, "running": {}}
        machine = StateMachine.from_dict(cfg, guard_dict={"guard_false": sync_guard_false})
        result = await machine.run("idle", Signal(event="START"), make_session())
        self.assertEqual(result, "idle")

    async def test_action_error_falls_back_to_error_state(self) -> None:
        cfg = {
            "idle": {"on": {"START": {"target": "running", "actions": ["boom"]}}, "error_state": "failed"},
            "running": {},
            "failed": {},
        }
        machine = StateMachine.from_dict(cfg, action_dict={"boom": action_raises})
        result = await machine.run("idle", Signal(event="START"), make_session())
        self.assertEqual(result, "failed")

    async def test_action_error_without_error_state_propagates(self) -> None:
        cfg = {"idle": {"on": {"START": {"target": "running", "actions": ["boom"]}}}, "running": {}}
        machine = StateMachine.from_dict(cfg, action_dict={"boom": action_raises})
        with self.assertRaises(TransitionError):
            await machine.run("idle", Signal(event="START"), make_session())


class TestEnter(unittest.IsolatedAsyncioTestCase):
    async def test_exit_and_entry_actions_fire(self) -> None:
        cfg = {"idle": {"exit": ["on_exit"]}, "running": {"entry": ["on_entry"]}}
        machine = StateMachine.from_dict(cfg, action_dict={"on_exit": sync_action, "on_entry": sync_action})
        ctx = ExecutionContext(current_state="idle", session=make_session())
        await machine._enter(ctx, "running", Signal(event="X"))
        self.assertEqual(ctx.current_state, "running")
        self.assertEqual(ctx.history, ["idle", "running"])
        self.assertEqual([r.name for r in ctx.results], ["on_exit", "on_entry"])
        self.assertEqual(ctx.results[0].source, "exit")
        self.assertEqual(ctx.results[1].source, "entry")

    async def test_enter_skips_execute_many_when_no_actions(self) -> None:
        cfg = {"idle": {}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        ctx = ExecutionContext(current_state="idle", session=make_session())
        await machine._enter(ctx, "running", Signal(event="X"))
        self.assertEqual(ctx.results, [])
        self.assertEqual(ctx.current_state, "running")


class TestCheckAlways(unittest.IsolatedAsyncioTestCase):
    async def test_no_always_returns_immediately(self) -> None:
        cfg = {"idle": {}}
        machine = StateMachine.from_dict(cfg)
        ctx = ExecutionContext(current_state="idle", session=make_session())
        await machine._check_always(ctx)
        self.assertEqual(ctx.current_state, "idle")
        self.assertEqual(ctx.results, [])

    async def test_guard_passes_transitions_and_restarts(self) -> None:
        cfg = {
            "idle": {"always": [{"target": "middle", "guard": "always_true", "actions": ["mark"]}]},
            "middle": {"always": [{"target": "end", "guard": "always_true"}]},
            "end": {},
        }
        machine = StateMachine.from_dict(
            cfg,
            guard_dict={"always_true": sync_guard_true},
            action_dict={"mark": sync_action},
        )
        ctx = ExecutionContext(current_state="idle", session=make_session())
        await machine._check_always(ctx)
        self.assertEqual(ctx.current_state, "end")
        self.assertEqual(ctx.history, ["idle", "middle", "end"])

    async def test_all_guards_false_returns_via_for_else(self) -> None:
        cfg = {"idle": {"always": [{"target": "idle", "guard": "always_false"}]}}
        machine = StateMachine.from_dict(cfg, guard_dict={"always_false": sync_guard_false})
        ctx = ExecutionContext(current_state="idle", session=make_session())
        await machine._check_always(ctx)
        self.assertEqual(ctx.current_state, "idle")

    async def test_loop_exceeding_max_depth_raises_runtimeerror(self) -> None:
        cfg = {
            "a": {"always": [{"target": "b", "guard": "always_true"}]},
            "b": {"always": [{"target": "a", "guard": "always_true"}]},
        }
        machine = StateMachine.from_dict(cfg, guard_dict={"always_true": sync_guard_true})
        ctx = ExecutionContext(current_state="a", session=make_session())
        with self.assertRaises(RuntimeError):
            await machine._check_always(ctx)


class TestTryTransitions(unittest.IsolatedAsyncioTestCase):
    async def test_second_candidate_fires_when_first_guard_fails(self) -> None:
        cfg = {
            "idle": {
                "on": {
                    "START": [
                        {"target": "blocked", "guard": "guard_false"},
                        {"target": "running", "guard": "guard_true"},
                    ]
                }
            },
            "blocked": {},
            "running": {},
        }
        machine = StateMachine.from_dict(
            cfg,
            guard_dict={"guard_false": sync_guard_false, "guard_true": sync_guard_true},
        )
        result = await machine.run("idle", Signal(event="START"), make_session())
        self.assertEqual(result, "running")

    async def test_all_candidates_fail_returns_false(self) -> None:
        cfg = {"idle": {"on": {"START": {"target": "running", "guard": "guard_false"}}}, "running": {}}
        machine = StateMachine.from_dict(cfg, guard_dict={"guard_false": sync_guard_false})
        ctx = ExecutionContext(current_state="idle", session=make_session())
        transitions = machine._transition_table[("idle", "START")]
        fired = await machine._try_transitions(ctx, transitions, Signal(event="START"))
        self.assertFalse(fired)
        self.assertEqual(ctx.current_state, "idle")

    async def test_action_raises_wrapped_as_transitionerror(self) -> None:
        cfg = {"idle": {"on": {"START": {"target": "running", "actions": ["boom"]}}}, "running": {}}
        machine = StateMachine.from_dict(cfg, action_dict={"boom": action_raises})
        ctx = ExecutionContext(current_state="idle", session=make_session())
        transitions = machine._transition_table[("idle", "START")]
        with self.assertRaises(TransitionError) as cm:
            await machine._try_transitions(ctx, transitions, Signal(event="START"))
        self.assertIsInstance(cm.exception.__cause__, ValueError)


class TestAvailableEvents(unittest.TestCase):
    def test_known_state(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        self.assertEqual(machine.available_events("idle"), ["START"])

    def test_unknown_state(self) -> None:
        cfg = {"idle": {}}
        machine = StateMachine.from_dict(cfg)
        self.assertEqual(machine.available_events("ghost"), [])


class TestRepr(unittest.TestCase):
    def test_repr_contains_state_names(self) -> None:
        cfg = {"idle": {}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        self.assertIn("idle", repr(machine))
        self.assertIn("running", repr(machine))


if __name__ == "__main__":
    unittest.main()
