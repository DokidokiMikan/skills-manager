---
name: skill-assistant
description: Consult the local Skills Manager skill library and recommend suitable skills for a user task.
---

# Skill Assistant

Use this skill when the user asks which local skill to use, how two skills differ,
or why a skill should or should not be selected for a task.

## Data Sources

Read `data/manifest.json` first. It records the source knowledge-base version,
package version, generation time, and the current data layout.

Use these files in order:

1. `data/index/basic-skills.json` for fast candidate discovery.
2. `data/enhancement-plan.json` to see which cards still need semantic enhancement.
3. `data/cards/*.json` for per-skill details.
4. `data/groups/*.json` for broad comparisons.
5. `references/*.md` for answer policy and update rules.

## Answering Policy

Be flexible rather than template-bound.

- If the user asks "which one should I use", give the recommendation first.
- If the user compares skills, explain the differences and the tradeoffs.
- If a skill is not a fit, say why.
- If the request is broad, provide likely candidates and ask for the missing detail.
- If a card has `enhancementStatus: "basic"`, treat its semantic fields as incomplete.

Do not invent local skills that are not present in the generated data.
Do not recommend skills marked as deleted or missing unless the user asks about history.
