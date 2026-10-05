# Agent Development Workflow

## Verification

Before reporting completion or committing changes to Rust code, dependencies, the toolchain, or formatting settings, run these commands from the repository root:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --locked --all-targets -- -D warnings
```

Fix failures before completing the task. If a check cannot run, report it as unverified and explain why. Clippy checks compilation for all targets and treats warnings as errors.

## Sandbox execution

When `.git` is read-only inside the sandbox, run Git writes such as `git add` and `git commit` outside the sandbox from the outset. Read-only Git commands can run inside the sandbox.
