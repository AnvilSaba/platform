## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues; use the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the default triage roles, mapped to readable GitHub labels with spaces. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repo using root `CONTEXT.md` and `docs/adr/`. See `docs/agents/domain.md`.

### Stacked pull requests

Use `gh stack` when implementing dependent GitHub issues.

- One issue per branch and PR.
- Name branches `feature/<topic>/issue-<number>-<slug>`.
- The parent specification is the bottom PR; stack implementation issues in dependency order.
- Include `Parent`, `Depends on`, and `Implements` issue references in each PR body as applicable.
