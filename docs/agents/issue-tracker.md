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

- Use `to-spec` to publish the parent specification issue. It is the source of truth for the feature's scope, behavior, and overall acceptance criteria.
- Use `to-tickets` (the installed skill name; sometimes called `to-ticket`) to split that specification into independently verifiable implementation issues after the user approves the breakdown.
- Register each implementation issue as a native GitHub sub-issue of the specification issue. A `Parent` reference in the body supplements this relationship; it does not replace it.
- Each child issue records its parent, the behavior it delivers, its own acceptance criteria, and `Blocked by` references. Reference the parent specification rather than copying it; surface any required specification change before publishing conflicting tickets.
- Track implementation dependencies separately from parent-child relationships. Use native GitHub blocking relationships where available and retain readable `Blocked by` references in the issue body. Being siblings does not imply execution order.
- Reuse existing parent and child issues when continuing a feature. Inspect existing sub-issues before publishing to avoid duplicates. If no parent specification exists, establish one before publishing child tickets rather than choosing an unrelated parent.
- Sub-issue registration may update the parent's relationship metadata. This is the only exception to `to-tickets`' instruction not to modify the parent: leave its title, body, labels, and state unchanged. Do not automatically close the parent when publishing or completing a child.
- Keep small standalone changes as a single issue when a specification and ticket breakdown are unnecessary.
- Write issue titles and bodies in Japanese. Apply the configured `ready for agent` label when publishing approved specifications and implementation tickets.
