# Domain Docs

This repository uses a single-context domain documentation layout.

## Before exploring

- Read `GLOSSARY.md` at the repository root when it exists.
- Read existing ADRs in `docs/adr/` that touch the area being changed.

## Vocabulary

When naming domain concepts in issue titles, refactor proposals, hypotheses, or tests, use the vocabulary defined in `GLOSSARY.md`. If an existing ADR conflicts with proposed work, surface the conflict explicitly rather than silently overriding it.

## Document locations

- Record domain terms in root `GLOSSARY.md` and long-term design decisions and their rationale in `docs/adr/`.
- Consult `docs/research/` for evidence about APIs, libraries, and existing code.
- Treat GitHub Issues as the source of truth for feature specifications and acceptance criteria; keep grilling drafts and question history out of permanent docs.
