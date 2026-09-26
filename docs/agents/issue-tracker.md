# Issue tracker: GitHub

Issues and specs for this repo live as GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`
- **Read an issue**: `gh issue view <number> --comments`
- **List issues**: `gh issue list --state open`
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

Infer the repository from the GitHub remote; `gh` does this automatically inside this clone.

## Pull requests as a triage surface

**PRs as a request surface: no.**

When a skill says **publish to the issue tracker**, create a GitHub issue. When it says **fetch the relevant ticket**, run `gh issue view <number> --comments`.
## Specs and implementation tickets

- The issue published by `to-spec` is the parent specification.
- Register issues published by `to-tickets` as native GitHub sub-issues of that specification. Updating the parent's relationship metadata is allowed; leave its content and state unchanged.
- Write issue titles and bodies in Japanese.
