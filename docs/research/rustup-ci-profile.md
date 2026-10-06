# rustup CI/profile override research

Scope: official `rust-lang/rustup` Issues/PRs, checked against live GitHub state on 2026-10-07. Search terms included `RUSTUP_PROFILE`, `profile`, `--profile`, `rust-toolchain.toml`, `CI`, and `CI=true`. Proposal status does not establish released functionality.

## Findings

- **Issue #2579 — “rust-toolchain should allow setting the equivalent of `--profile minimal`” — closed/completed (2020-12-15).** The discussion initially identified global `rustup set profile` as the existing workaround, then proposed a TOML `profile` field. **PR #2586 — “Profile in rust toolchain” — closed/merged (2020-12-15)** implemented that TOML field. [#2579](https://github.com/rust-lang/rustup/issues/2579) · [#2586](https://github.com/rust-lang/rustup/pull/2586)

- **Issue #3805 — “rustup should use the configured profile as fallback…” — closed/completed (2024-10-03).** [PR #4040](https://github.com/rust-lang/rustup/pull/4040) added the regression test and closed it; the conclusion was that an omitted file profile should fall back to the configured rustup profile. The current book says the same: omission does not necessarily mean `default`; `rustup set profile` may supply the default. [#3805](https://github.com/rust-lang/rustup/issues/3805) · [book](https://rust-lang.github.io/rustup/overrides.html#profile)

- **Issue #5011 — “Provide a built-in way for rustup to adapt to a CI environment” — open, opened 2026-08-11.** It explores bringing third-party setup-action behavior into rustup. Automatically choosing `minimal` in CI was discussed, but compatibility concerns led to an opt-in command suggestion such as `rustup setup CI`; this was not a finalized API. [#5011](https://github.com/rust-lang/rustup/issues/5011) · [minimal discussion](https://github.com/rust-lang/rustup/issues/5011#issuecomment-5257694200) · [opt-in suggestion](https://github.com/rust-lang/rustup/issues/5011#issuecomment-5257837607)

- **No confirmed `RUSTUP_PROFILE` (or equivalent) environment-variable proposal was found.** #5011 discusses CI defaults and an opt-in `rustup setup CI`, but does not define such a variable. [#5011](https://github.com/rust-lang/rustup/issues/5011)

- **Issue #3078 — “Reconsider recommending `--profile minimal` for CI (or add CI tools to the minimal profile)” — open, opened 2022-10-04.** Proposals include a CI profile with rustfmt/clippy, effectively `default` without documentation. Participants disagree because CI requirements vary; no generic CI profile was finalized. [#3078](https://github.com/rust-lang/rustup/issues/3078) · [counterargument](https://github.com/rust-lang/rustup/issues/3078#issuecomment-1463806508)

- **PR #5021** (“feat(cli/rustup-mode): add `rustup ci` subcommand”) is open, linked to #5011; its body retreats `rustup ci env` and retains only GitHub problem-matchers. It contains no profile/minimal implementation. [#5021](https://github.com/rust-lang/rustup/pull/5021)
