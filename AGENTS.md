## Language and execution environment

- Write user-facing plain text in Japanese.
- Write agent-facing documentation in English.
- Always run `gh` commands, including `mise exec -- gh ...`, outside the sandbox. Do not retry `gh` inside the sandbox after a network or authentication failure.

## Development

- Run project-managed tools through mise tasks or `mise exec -- <command>`. Apply this rule to documentation examples and skill commands even when they omit the wrapper. Git is a system prerequisite and can be invoked directly.

- Before completing or committing code, configuration, documentation, dependency, or toolchain changes, follow the applicable steps in `docs/agents/development.md`.
- For Git writes with read-only sandbox access to `.git`, follow the execution guidance in `docs/agents/development.md`.
- GitHub Actions dependencies are managed with `gh-actions-lock`. When changing workflows or composite actions, consult the [official README](https://raw.githubusercontent.com/github/gh-actions-lock/refs/heads/main/README.md).

## Commit and PR titles

- Follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) for commit messages and PR titles, using previous commits and PRs as examples.
- Write the summary in Japanese, for example: `feat(scope): 日本語の要約`.

## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues; use the `gh` CLI. When running `to-spec` or `to-tickets`, follow the parent-specification and native sub-issue workflow in `docs/agents/issue-tracker.md`.

### Triage labels

Use the default triage roles, mapped to readable GitHub labels with spaces. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repo using root `GLOSSARY.md` and `docs/adr/`. See `docs/agents/domain.md`.

### Stacked pull requests

Use `gh stack` when implementing dependent GitHub issues.

- One issue per branch and PR.
- Name branches `feature/<topic>/issue-<number>-<slug>`.
- The parent specification is the bottom PR; stack implementation issues in dependency order.
- Include `Parent`, `Depends on`, and `Implements` issue references in each PR body as applicable.
