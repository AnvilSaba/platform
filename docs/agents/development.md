# Agent Development Workflow

## Verification

After changing files supported by rustfmt or Oxfmt, run `mise :fmt` and then `mise :fmt:check` from the repository root before completing or committing the task. Run `mise install` to install tools and npm dependencies. Invoke tasks directly as `mise :<task>` or `mise //<path>:<task>`. mise also checks npm dependencies before task execution and `mise exec`.

For Rust code, Rust dependencies, the Rust toolchain, or formatting settings, also run:

```powershell
cargo clippy --workspace --locked --all-targets -- -D warnings
```

Fix failures before completing the task. If a check cannot run, report it as unverified and explain why. Clippy checks compilation for all targets and treats warnings as errors.

## Database changes

- Add migrations; leave applied files unchanged. Preserve running and rollback compatibility; see `docs/releases.md`.
- After SQL or schema changes, run `mise :sqlx:prepare` and `mise :sqlx:check`; commit `.sqlx/` updates.
- Update each affected app's required migration IDs, including transitive dependencies; verify the list during review.
- Grant runtime roles only required table and column privileges, without database ownership or schema creation rights. Reserve schema changes for the Migrator Job.

## Sandbox execution

When `.git` is read-only inside the sandbox, run Git writes such as `git add` and `git commit` outside the sandbox from the outset. Read-only Git commands can run inside the sandbox.
