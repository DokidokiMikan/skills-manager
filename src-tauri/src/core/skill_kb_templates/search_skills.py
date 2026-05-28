#!/usr/bin/env python3
"""Lightweight local search helper for generated skill-assistant data."""

from __future__ import annotations

import json
import sys
from pathlib import Path


def load_index(root: Path) -> list[dict]:
    index_path = root / "data" / "index" / "basic-skills.json"
    data = json.loads(index_path.read_text(encoding="utf-8"))
    return data.get("skills", [])


def score_skill(skill: dict, terms: list[str]) -> int:
    haystack = " ".join(
        str(skill.get(key) or "")
        for key in ("id", "name", "description", "status")
    ).lower()
    return sum(1 for term in terms if term in haystack)


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    query = " ".join(sys.argv[1:]).strip().lower()
    if not query:
        print("Usage: search_skills.py <query>", file=sys.stderr)
        return 2

    terms = [term for term in query.split() if term]
    matches = [
        (score_skill(skill, terms), skill)
        for skill in load_index(root)
    ]
    for score, skill in sorted(matches, key=lambda item: (-item[0], item[1].get("name", "")))[:10]:
        if score <= 0:
            continue
        print(f"{skill.get('name')} [{skill.get('id')}] -> {skill.get('cardPath')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
