#!/usr/bin/env python3
"""Check all 76 recorded decisions and checkbox/evidence consistency.

Exit success means the documents agree, NOT that the renderer has full parity.
Human/source validation is still required for each Implemented decision.
"""
import json
import re
from pathlib import Path

COUNTS = {1: 12, 2: 9, 3: 9, 4: 9, 5: 9, 6: 9, 7: 9, 8: 10}
REQUIRED = {f"P{phase}.{i:02}" for phase, count in COUNTS.items() for i in range(1, count + 1)}
STATES = {"Implemented", "Partial", "Not implemented"}


def audit(checklist, assessment, root=None):
    errors = []
    decisions = {}
    checked = {}
    for task, state in re.findall(r"^\|\s*(P\d+\.\d+)\s*\|\s*([^|]+?)\s*\|", assessment, re.M):
        if task in decisions:
            errors.append(f"duplicate decision: {task}")
        if task not in REQUIRED:
            errors.append(f"unexpected decision: {task}")
        if state not in STATES:
            errors.append(f"invalid decision for {task}: {state}")
        decisions[task] = state
    for mark, task in re.findall(r"^- \[([ x])\]\s+(P\d+\.\d+)\b", checklist, re.M):
        if task in checked:
            errors.append(f"duplicate checkbox: {task}")
        if task not in REQUIRED:
            errors.append(f"unexpected checkbox: {task}")
        checked[task] = mark == "x"
    for task in sorted(REQUIRED):
        if task not in decisions:
            errors.append(f"missing decision: {task}")
        if task not in checked:
            errors.append(f"missing checkbox: {task}")
        if task in checked and task in decisions and checked[task] != (decisions[task] == "Implemented"):
            errors.append(f"checkbox/decision mismatch: {task}")
    if root is not None:
        paths = set(re.findall(r"`((?:src/|tools/|Findings/|\.github/)[^`]+)`", assessment))
        for path in sorted(paths):
            if not (root / path).is_file():
                errors.append(f"missing evidence file: {path}")
    counts = {state: sum(v == state for k, v in decisions.items() if k in REQUIRED) for state in sorted(STATES)}
    complete_phases = [phase for phase, count in COUNTS.items()
                       if all(decisions.get(f"P{phase}.{i:02}") == "Implemented" for i in range(1, count + 1))]
    return {"consistent": not errors, "counts": counts, "complete_phases": complete_phases, "errors": errors}


def main():
    root = Path(__file__).resolve().parents[1]
    result = audit((root / "Findings/Eight-phase-status.md").read_text(),
                   (root / "Findings/Eight-phase-reassessment-2026-11-02.md").read_text(), root)
    print(json.dumps(result, indent=2))
    return 0 if result["consistent"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
