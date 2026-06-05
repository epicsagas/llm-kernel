# Contributing to ec-kernel

Thank you for your interest in contributing! This project follows the [EpicCounty contribution guidelines](https://github.com/epicsagas).

## Development Setup

```bash
git clone https://github.com/epicsagas/ec-kernel.git
cd ec-kernel
cargo build
cargo test
```

## Development Workflow

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Make your changes
4. Ensure tests pass: `cargo test`
5. Ensure linting passes: `cargo clippy -- -D warnings`
6. Ensure formatting: `cargo fmt`
7. Commit with [Conventional Commits](https://www.conventionalcommits.org/): `feat(scope): description`
8. Open a Pull Request

## Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — New feature
- `fix:` — Bug fix
- `docs:` — Documentation changes
- `refactor:` — Code refactoring
- `test:` — Adding or updating tests
- `chore:` — Build, CI, or tooling changes

## Code Style

- Follow `cargo fmt` formatting
- Resolve all `cargo clippy` warnings
- Add tests for new functionality
- Keep public API minimal and well-documented

## Reporting Issues

- Use [GitHub Issues](https://github.com/epicsagas/ec-kernel/issues)
- Include reproduction steps
- Specify Rust version (`rustc --version`)

## License

By contributing, you agree that your contributions will be licensed under the [Apache-2.0 License](LICENSE).
