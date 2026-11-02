# team

- CI gates: `cargo clippy --features full -- -D warnings`, `cargo clippy --no-default-features -- -D warnings`, `cargo fmt --all -- --check`, `cargo test --features full`, `cargo doc --features full --no-deps`. Run the matching subset before reporting any change as done.
- Domain modules (`provider`, `tokens`, `embedding` base) stay zero-dependency. A new external crate in one of them is a design decision, not an implementation detail: stop and ask.
- Commits follow Conventional Commits (`type(scope): description`). Version bumps follow the checklist in AGENTS.md and are their own `chore: bump vX.Y.Z` commit.
- Never bypass hooks. `--no-verify` is not an option.
- file: CONTRIBUTING.md

## spec
- Every spec names the feature gate(s) touched and states whether any new external dependency is introduced.
