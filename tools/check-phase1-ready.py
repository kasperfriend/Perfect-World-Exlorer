#!/usr/bin/env python3
"""Fail closed on Phase 1's full-scope acceptance table, independently of builds.

This checks recorded acceptance decisions, not renderer parity by itself.
Completion approval requires this gate and validation to pass.
A failed checkpoint must report No and continue implementation.
"""
import json
import re
import sys
from pathlib import Path

REQUIRED = {f"P1.{i:02}" for i in range(1, 13)}


def audit(text):
    rows = re.findall(r"^\|\s*(P1\.\d{2})(?:\s+[^|]*?)?\s*\|\s*([^|]+?)\s*\|", text, re.M)
    statuses = {}
    errors = []
    for task, status in rows:
        if task in statuses:
            errors.append(f"duplicate task: {task}")
        if task not in REQUIRED:
            errors.append(f"unexpected task: {task}")
        if status != "No" and not re.fullmatch(r"Yes(?:, .+)?", status):
            errors.append(f"invalid status for {task}: {status}")
        statuses[task] = status
    for task in sorted(REQUIRED - statuses.keys()):
        errors.append(f"missing task: {task}")
    unfinished = sorted(task for task in REQUIRED if not re.fullmatch(r"Yes(?:, .+)?", statuses.get(task, "No")))
    return {"ready": not errors and not unfinished, "unfinished": unfinished, "errors": errors, "statuses": statuses}


def main():
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "Findings/Phase-1-completion-checks.md"
    try:
        result = audit(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError) as error:
        result = {"ready": False, "unfinished": sorted(REQUIRED), "errors": [str(error)], "statuses": {}}
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0 if result["ready"] else 1


if __name__ == "__main__":
    sys.exit(main())
