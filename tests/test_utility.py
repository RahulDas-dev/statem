from __future__ import annotations

import unittest

from statem import ExecutionContext, ResultEntry, show_transitions
from statem.utility import _DIVIDER
from tests.helpers import make_session


class TestShowTransitions(unittest.TestCase):
    def test_no_transitions_zero_hops_plural(self) -> None:
        ctx = ExecutionContext(current_state="idle", session=make_session())
        output = show_transitions(ctx)
        self.assertTrue(output.startswith("Transitions (0 hops):"))
        self.assertIn("Final state: idle", output)
        self.assertNotIn("hop 1:", output)

    def test_single_hop_singular_and_source_dedup(self) -> None:
        ctx = ExecutionContext(current_state="running", session=make_session())
        ctx.history = ["idle", "running"]
        ctx.results = [
            ResultEntry(state="idle", source="on", kind="guard", name="g1", value="rawvalue"),
            ResultEntry(state="idle", source="on", kind="action", name="a1", value="rawvalue"),
            ResultEntry(state="idle", source="entry", kind="action", name="a2", value=None),
        ]

        output = show_transitions(ctx)

        self.assertIn("Transitions (1 hop):", output)
        self.assertNotIn("1 hops", output)
        self.assertIn("hop 1: idle -> running", output)
        self.assertIn("Final state: running", output)

        lines = {line.strip(): line for line in output.splitlines() if line.strip()}
        guard_line = next(v for k, v in lines.items() if "g1" in k)
        action_line = next(v for k, v in lines.items() if "a1" in k)
        entry_line = next(v for k, v in lines.items() if "a2" in k)

        self.assertTrue(guard_line.strip().startswith("on"))
        self.assertIn("= rawvalue", guard_line)

        self.assertFalse(action_line.strip().startswith("on"))
        self.assertTrue(action_line.strip().startswith("action"))
        self.assertIn("= 'rawvalue'", action_line)

        self.assertTrue(entry_line.strip().startswith("entry"))

    def test_multi_hop_plural_and_empty_state_section(self) -> None:
        ctx = ExecutionContext(current_state="c", session=make_session())
        ctx.history = ["a", "b", "c"]
        ctx.results = [ResultEntry(state="a", source="on", kind="action", name="act", value=None)]

        output = show_transitions(ctx)

        self.assertIn("Transitions (2 hops):", output)
        self.assertIn("hop 1: a -> b", output)
        self.assertIn("hop 2: b -> c", output)
        self.assertIn("Final state: c", output)
        self.assertEqual(output.count(_DIVIDER), 4)


if __name__ == "__main__":
    unittest.main()
