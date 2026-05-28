# Update Guide

The package is generated from Skills Manager's local knowledge base.

Routine update flow:

1. Refresh the Skills Manager knowledge base.
2. Check `data/manifest.json` for the `sourceKbVersion`.
3. Use `data/index/basic-skills.json` to identify added or changed skills.
4. Update only affected `data/cards/*.json` files when possible.
5. Keep `SKILL.md` lightweight and stable.

Semantic enhancement is optional. A basic card should keep
`enhancementStatus: "basic"` until a model-generated card has been reviewed or
accepted by the user.

Never delete or edit the source skills while updating this generated assistant.
This package is derived data; the original local skill library remains the
source of truth.
