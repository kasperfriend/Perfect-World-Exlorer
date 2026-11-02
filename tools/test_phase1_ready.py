#!/usr/bin/env python3
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("ready", Path(__file__).with_name("check-phase1-ready.py"))
ready = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ready)


def table(status="Yes"):
    return "\n".join(f"| P1.{i:02} | {status} | evidence |" for i in range(1, 13))


class ReadinessTests(unittest.TestCase):
    def test_all_twelve_affirmative_decisions_required(self):
        self.assertTrue(ready.audit(table())["ready"])
        self.assertFalse(ready.audit("")["ready"])

    def test_titled_tasks_match_the_actual_acceptance_table(self):
        titled=table().replace("P1.01 |", "P1.01 Complete scene-light selection |")
        self.assertTrue(ready.audit(titled)["ready"])
        actual=Path(__file__).resolve().parents[1]/"Findings/Phase-1-completion-checks.md"
        self.assertFalse(ready.audit(actual.read_text())["errors"])

    def test_one_no_blocks_confirmation(self):
        result = ready.audit(table().replace("P1.03 | Yes", "P1.03 | No"))
        self.assertFalse(result["ready"])
        self.assertEqual(result["unfinished"], ["P1.03"])

    def test_missing_row_cannot_make_open_list_empty(self):
        result = ready.audit("\n".join(table().splitlines()[1:]))
        self.assertFalse(result["ready"])
        self.assertIn("missing task: P1.01", result["errors"])

    def test_duplicate_or_unknown_task_fails(self):
        for extra in ["| P1.01 | Yes | duplicate |", "| P1.13 | Yes | extra |"]:
            self.assertFalse(ready.audit(table() + "\n" + extra)["ready"])

    def test_unknown_status_is_not_accepted_as_yes(self):
        for status in ["Ready", "Yes-ish", "Maybe", "No, but CI passed"]:
            self.assertFalse(ready.audit(table(status))["ready"])

    def test_qualified_preservation_and_coverage_decisions(self):
        self.assertTrue(ready.audit(table("Yes, category coverage"))["ready"])


if __name__ == "__main__":
    unittest.main()
