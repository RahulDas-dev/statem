from __future__ import annotations

import unittest

from ag_ui.core import EventType

from statem import Signal, StateMachine, TransitionError
from tests.helpers import (
    action_raises,
    make_session,
    sync_action,
    sync_guard_false,
    sync_guard_true,
)


async def _collect(machine: StateMachine, **kwargs: object) -> list[object]:
    return [event async for event in machine.stream(**kwargs)]


def _bump_counter(ctx: object, signal: object) -> None:
    ctx.session["counter"] += 1


class TestStream(unittest.IsolatedAsyncioTestCase):
    async def test_single_signal_event_sequence(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        events = await _collect(machine, state_name="idle", events=Signal(event="START"), session=make_session())

        types = [e.type for e in events]
        self.assertEqual(
            types,
            [
                EventType.STEP_STARTED,
                EventType.STATE_SNAPSHOT,
                EventType.STATE_DELTA,
                EventType.STEP_FINISHED,
            ],
        )
        self.assertEqual(events[0].step_name, "START")
        self.assertEqual(events[1].snapshot, {"current_state": "idle"})
        self.assertEqual(events[2].delta, [{"op": "replace", "path": "/current_state", "value": "running"}])
        self.assertEqual(events[3].step_name, "START")

    async def test_no_run_events_ever_emitted(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        events = await _collect(machine, state_name="idle", events=Signal(event="START"), session=make_session())
        types = {e.type for e in events}
        self.assertNotIn(EventType.RUN_STARTED, types)
        self.assertNotIn(EventType.RUN_FINISHED, types)
        self.assertNotIn(EventType.RUN_ERROR, types)

    async def test_guard_and_action_emit_activity_snapshot(self) -> None:
        cfg = {
            "idle": {"on": {"START": {"target": "running", "guard": "ok", "actions": ["act"]}}},
            "running": {},
        }
        machine = StateMachine.from_dict(cfg, guard_dict={"ok": sync_guard_true}, action_dict={"act": sync_action})
        events = await _collect(machine, state_name="idle", events=Signal(event="START"), session=make_session())

        activities = [e for e in events if e.type == EventType.ACTIVITY_SNAPSHOT]
        self.assertEqual(len(activities), 2)

        guard_evt, action_evt = activities
        self.assertEqual(guard_evt.activity_type, "guard")
        self.assertEqual(
            guard_evt.content, {"type": "activity", "kind": "guard", "name": "ok", "source": "on", "result": True}
        )
        self.assertEqual(action_evt.activity_type, "action")
        self.assertEqual(
            action_evt.content, {"type": "activity", "kind": "action", "name": "act", "source": "on", "result": None}
        )

    async def test_guardless_transition_emits_no_guard_activity(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        events = await _collect(machine, state_name="idle", events=Signal(event="START"), session=make_session())
        self.assertEqual([e for e in events if e.type == EventType.ACTIVITY_SNAPSHOT], [])

    async def test_always_cascade_emits_state_delta_and_guard_activity_per_hop(self) -> None:
        cfg = {
            "idle": {"always": [{"target": "mid", "guard": "always_true"}]},
            "mid": {"always": [{"target": "done", "guard": "always_true"}]},
            "done": {},
        }
        machine = StateMachine.from_dict(cfg, guard_dict={"always_true": sync_guard_true})
        events = await _collect(machine, state_name="idle", events=[], session=make_session())

        deltas = [e.delta[0]["value"] for e in events if e.type == EventType.STATE_DELTA]
        self.assertEqual(deltas, ["mid", "done"])

        guard_activities = [e for e in events if e.type == EventType.ACTIVITY_SNAPSHOT]
        self.assertEqual([a.content["result"] for a in guard_activities], [True, True])

    async def test_failed_guard_still_reports_activity(self) -> None:
        cfg = {
            "idle": {
                "on": {
                    "START": [
                        {"target": "blocked", "guard": "no"},
                        {"target": "running"},
                    ]
                }
            },
            "blocked": {},
            "running": {},
        }
        machine = StateMachine.from_dict(cfg, guard_dict={"no": sync_guard_false})
        events = await _collect(machine, state_name="idle", events=Signal(event="START"), session=make_session())
        activities = [e for e in events if e.type == EventType.ACTIVITY_SNAPSHOT]
        self.assertEqual(len(activities), 1)
        self.assertEqual(
            activities[0].content, {"type": "activity", "kind": "guard", "name": "no", "source": "on", "result": False}
        )
        deltas = [e for e in events if e.type == EventType.STATE_DELTA]
        self.assertEqual(deltas[0].delta[0]["value"], "running")

    async def test_multi_hop_signal_gets_one_step_per_hop(self) -> None:
        cfg = {
            "idle": {"on": {"START": "mid"}},
            "mid": {"always": [{"target": "done", "guard": "always_true"}]},
            "done": {},
        }
        machine = StateMachine.from_dict(cfg, guard_dict={"always_true": sync_guard_true})
        events = await _collect(machine, state_name="idle", events=Signal(event="START"), session=make_session())

        types = [e.type for e in events]
        self.assertEqual(
            types,
            [
                EventType.STEP_STARTED,
                EventType.STATE_SNAPSHOT,
                EventType.STATE_DELTA,
                EventType.STEP_FINISHED,
                EventType.STEP_STARTED,
                EventType.STATE_SNAPSHOT,
                EventType.ACTIVITY_SNAPSHOT,
                EventType.STATE_DELTA,
                EventType.STEP_FINISHED,
            ],
        )
        self.assertEqual(events[0].step_name, "START")
        self.assertEqual(events[1].snapshot, {"current_state": "idle"})
        self.assertEqual(events[2].delta, [{"op": "replace", "path": "/current_state", "value": "mid"}])
        self.assertEqual(events[4].step_name, "__always__")
        self.assertEqual(events[5].snapshot, {"current_state": "mid"})
        self.assertEqual(events[7].delta, [{"op": "replace", "path": "/current_state", "value": "done"}])

    async def test_no_hop_on_all_guards_failing_still_opens_and_closes_step(self) -> None:
        cfg = {
            "idle": {"on": {"START": {"target": "running", "guard": "no"}}},
            "running": {},
        }
        machine = StateMachine.from_dict(cfg, guard_dict={"no": sync_guard_false})
        events = await _collect(machine, state_name="idle", events=Signal(event="START"), session=make_session())

        types = [e.type for e in events]
        self.assertEqual(
            types,
            [
                EventType.STEP_STARTED,
                EventType.STATE_SNAPSHOT,
                EventType.ACTIVITY_SNAPSHOT,
                EventType.STEP_FINISHED,
            ],
        )

    async def test_final_state_matches_equivalent_run(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {"on": {"STOP": "idle"}}}
        machine = StateMachine.from_dict(cfg)
        signals = [Signal(event="START"), Signal(event="STOP")]

        run_result = await machine.run(state_name="idle", events=signals, session=make_session())
        events = await _collect(machine, state_name="idle", events=signals, session=make_session())

        last_delta = next(e for e in reversed(events) if e.type == EventType.STATE_DELTA)
        self.assertEqual(last_delta.delta[0]["value"], run_result)

    async def test_unhandled_action_error_propagates_from_generator(self) -> None:
        cfg = {"idle": {"on": {"START": {"target": "running", "actions": ["boom"]}}}, "running": {}}
        machine = StateMachine.from_dict(cfg, action_dict={"boom": action_raises})
        with self.assertRaises(TransitionError):
            await _collect(machine, state_name="idle", events=Signal(event="START"), session=make_session())

    async def test_state_accessor_drives_snapshot_and_delta(self) -> None:
        cfg = {"idle": {"on": {"START": {"target": "running", "actions": ["bump"]}}}, "running": {}}
        machine = StateMachine.from_dict(cfg, action_dict={"bump": _bump_counter})
        session = {"counter": 0}
        events = await _collect(
            machine,
            state_name="idle",
            events=Signal(event="START"),
            session=session,
            state_accessor=lambda s: {"counter": s["counter"]},
        )

        snapshot = next(e for e in events if e.type == EventType.STATE_SNAPSHOT)
        self.assertEqual(snapshot.snapshot, {"counter": 0})

        delta = next(e for e in events if e.type == EventType.STATE_DELTA)
        self.assertEqual(delta.delta, [{"op": "replace", "path": "/counter", "value": 1}])

    async def test_state_accessor_unchanged_output_emits_no_delta(self) -> None:
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        events = await _collect(
            machine,
            state_name="idle",
            events=Signal(event="START"),
            session=make_session(),
            state_accessor=lambda _s: {"fixed": "value"},
        )
        self.assertEqual([e for e in events if e.type == EventType.STATE_DELTA], [])
        types = [e.type for e in events]
        self.assertEqual(
            types,
            [EventType.STEP_STARTED, EventType.STATE_SNAPSHOT, EventType.STEP_FINISHED],
        )

    async def test_run_still_works_unaffected(self) -> None:
        """`run()` behavior/return value must be untouched by adding `stream()`."""
        cfg = {"idle": {"on": {"START": "running"}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        result = await machine.run(state_name="idle", events=Signal(event="START"), session=make_session())
        self.assertEqual(result, "running")


if __name__ == "__main__":
    unittest.main()
