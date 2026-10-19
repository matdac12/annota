# Issue tracker: Traccia

Issues and specs for this repo live in Traccia (Mattia's self-hosted tracker, on a private tailnet), not on GitHub. Use the `traccia` MCP tools; the server URL lives in your local MCP configuration, not in the repo. Writes are recorded as the `agent` actor.

- Project: `Annota`, key `ANNOTA`. Milestone `v1` holds the v1 build. File every issue in the project.
- The planning map is "Map: Annota v1" (`ANNOTA-1`, label `wayfinder:map`); its children labelled `wayfinder:*` are decisions, not build work.
- The spec is the project document `annota-v1-spec.md`.

## Conventions

- **Search before you create**: `list_issues` with `query` and `project: ANNOTA`; comment on an existing issue instead of filing a duplicate.
- **Create**: `save_issue` with `project: ANNOTA`, `milestone: v1` for v1 work, a title and a markdown description (real newlines).
- **Read**: `get_issue` by identifier (e.g. `ANNOTA-12`); comments and relations are included.
- **Labels**: `save_issue` with the full desired label set; labels must already exist (`list_issue_labels`, see `triage-labels.md`). Area labels in use: `backend` (Rust), `frontend` (Editor page), `infra` (build, installer, CI), `docs`.
- **Status**: `In Progress` when you start, `In Review` when a PR is ready, `Done` only once verified, `Canceled` for `wontfix`.
- **Blocking**: Traccia's native `blockedBy`/`blocks`. Take only issues whose blockers are all `Done`.
- Reference issues by identifier (`ANNOTA-12`) in branch names, commits, PR titles and comments.

## Knowledge

Check `list_memories` and `list_documents` for project `ANNOTA` before starting; record what the next agent should know with `save_memory` (update rather than duplicate).

## When a skill says "publish to the issue tracker"

Create a Traccia issue in project `ANNOTA`.

## When a skill says "fetch the relevant ticket"

Call `get_issue` with the identifier.
