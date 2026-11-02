#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("status", Path(__file__).with_name("check-eight-phase-status.py"))
status = importlib.util.module_from_spec(spec)
spec.loader.exec_module(status)


def documents():
    ids = sorted(status.REQUIRED)
    return ("\n".join(f"- [ ] {task} — requirement" for task in ids),
            "\n".join(f"| {task} | Partial | evidence | gap |" for task in ids))


class StatusTests(unittest.TestCase):
    def test_partial_documents_can_be_consistent_without_being_complete(self):
        result = status.audit(*documents())
        self.assertTrue(result["consistent"])
        self.assertEqual(result["counts"]["Partial"], 76)
        self.assertEqual(result["complete_phases"], [])

    def test_only_implemented_deliverables_can_be_checked(self):
        checklist, assessment = documents()
        checklist = checklist.replace("[ ] P4.08", "[x] P4.08")
        self.assertFalse(status.audit(checklist, assessment)["consistent"])
        assessment = assessment.replace("P4.08 | Partial", "P4.08 | Implemented")
        self.assertTrue(status.audit(checklist, assessment)["consistent"])

    def test_missing_duplicate_unexpected_and_invalid_fail_closed(self):
        checklist, assessment = documents()
        for changed in [assessment.replace("| P1.01 |", "| P9.01 |"),
                        assessment + "\n| P1.01 | Partial | duplicate | gap |",
                        assessment.replace("P1.01 | Partial", "P1.01 | Maybe")]:
            self.assertFalse(status.audit(checklist, changed)["consistent"])
        self.assertFalse(status.audit(checklist.replace("- [ ] P1.01", "not a checkbox"), assessment)["consistent"])
        self.assertFalse(status.audit(checklist + "\n- [ ] P1.01", assessment)["consistent"])

    def test_missing_evidence_file_is_reported(self):
        checklist, assessment = documents()
        result = status.audit(checklist, assessment + "\n`src/definitely-not-an-evidence-file.rs`", Path(__file__).resolve().parents[1])
        self.assertFalse(result["consistent"])
        self.assertTrue(any("missing evidence file" in e for e in result["errors"]))

    def test_current_documents_cover_every_requirement(self):
        root = Path(__file__).resolve().parents[1]
        result = status.audit((root / "Findings/Eight-phase-status.md").read_text(),
                              (root / "Findings/Eight-phase-reassessment-2026-11-02.md").read_text(), root)
        self.assertTrue(result["consistent"], result["errors"])


if __name__ == "__main__":
    unittest.main()
