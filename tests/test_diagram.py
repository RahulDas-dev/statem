from __future__ import annotations

import unittest

from statem import StateMachine, to_mermaid


class TestToMermaid(unittest.TestCase):
    def test_header_line(self) -> None:
        machine = StateMachine.from_dict({"idle": {}})
        output = to_mermaid(machine)
        self.assertEqual(output.splitlines()[0], "stateDiagram-v2")

    def test_on_transition_without_guard(self) -> None:
        machine = StateMachine.from_dict({"idle": {"on": {"START": "running"}}, "running": {}})
        output = to_mermaid(machine)
        self.assertIn("idle --> running: START", output)

    def test_on_transition_with_guard(self) -> None:
        cfg = {"idle": {"on": {"START": {"target": "running", "guard": "g"}}}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        output = to_mermaid(machine)
        self.assertIn("idle --> running: START [g]", output)

    def test_guard_chain_produces_multiple_labeled_edges(self) -> None:
        cfg = {
            "idle": {
                "on": {
                    "GO": [
                        {"target": "a", "guard": "g1"},
                        {"target": "b", "guard": "g2"},
                    ]
                }
            },
            "a": {},
            "b": {},
        }
        machine = StateMachine.from_dict(cfg)
        output = to_mermaid(machine)
        self.assertIn("idle --> a: GO [g1]", output)
        self.assertIn("idle --> b: GO [g2]", output)

    def test_wildcard_event_label(self) -> None:
        machine = StateMachine.from_dict({"idle": {"on": {"*": "running"}}, "running": {}})
        output = to_mermaid(machine)
        self.assertIn("idle --> running: *", output)

    def test_always_without_guard(self) -> None:
        machine = StateMachine.from_dict({"idle": {"always": [{"target": "running"}]}, "running": {}})
        output = to_mermaid(machine)
        self.assertIn("idle --> running: always", output)
        self.assertNotIn("always [", output)

    def test_always_with_guard(self) -> None:
        cfg = {"idle": {"always": [{"target": "running", "guard": "g"}]}, "running": {}}
        machine = StateMachine.from_dict(cfg)
        output = to_mermaid(machine)
        self.assertIn("idle --> running: always [g]", output)

    def test_error_state_edge(self) -> None:
        machine = StateMachine.from_dict({"idle": {"error_state": "failed"}, "failed": {}})
        output = to_mermaid(machine)
        self.assertIn("idle --> failed: error", output)

    def test_no_error_state_no_error_edge(self) -> None:
        machine = StateMachine.from_dict({"idle": {}})
        output = to_mermaid(machine)
        self.assertNotIn(": error", output)

    def test_initial_present_prepends_entry_edge(self) -> None:
        machine = StateMachine.from_dict({"idle": {"on": {"START": "running"}}, "running": {}})
        output = to_mermaid(machine, initial="idle")
        self.assertIn("[*] --> idle", output)

    def test_initial_omitted_no_entry_edge(self) -> None:
        machine = StateMachine.from_dict({"idle": {}})
        output = to_mermaid(machine)
        self.assertNotIn("[*]", output)

    def test_initial_unknown_state_ignored(self) -> None:
        machine = StateMachine.from_dict({"idle": {}})
        output = to_mermaid(machine, initial="ghost")
        self.assertNotIn("[*]", output)

    def test_state_name_sanitized_and_aliased(self) -> None:
        machine = StateMachine.from_dict({"my state": {"on": {"GO": "other-state"}}, "other-state": {}})
        output = to_mermaid(machine)
        self.assertIn('state "my state" as my_state', output)
        self.assertIn('state "other-state" as other_state', output)
        self.assertIn("my_state --> other_state: GO", output)

    def test_already_safe_name_gets_no_alias_line(self) -> None:
        machine = StateMachine.from_dict({"idle": {}})
        output = to_mermaid(machine)
        self.assertNotIn('state "idle" as', output)

    def test_leading_digit_name_gets_prefixed(self) -> None:
        machine = StateMachine.from_dict({"1abc": {}})
        output = to_mermaid(machine)
        self.assertIn('state "1abc" as s_1abc', output)

    def test_colliding_sanitized_ids_get_unique_suffix(self) -> None:
        machine = StateMachine.from_dict({"a b": {"on": {"GO": "a-b"}}, "a-b": {}})
        output = to_mermaid(machine)
        self.assertIn('state "a b" as a_b', output)
        self.assertIn('state "a-b" as a_b_2', output)
        self.assertIn("a_b --> a_b_2: GO", output)


if __name__ == "__main__":
    unittest.main()
