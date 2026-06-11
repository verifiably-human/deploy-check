# Contributing to deploy-check

Thanks for considering a contribution. This document covers how to set
up the repository, the conventions the code follows, and how to submit
changes.

## Prerequisites

- [Rust](https://rustup.rs) ≥ 1.88
- A compactc build of a contract is helpful for end-to-end testing but
  not required for unit work.

## Building

```sh
cargo build
```

The `midnight-ledger-v9` dependency is pulled from
[midnightntwrk/midnight-ledger](https://github.com/midnightntwrk/midnight-ledger)
at a pinned commit declared in `Cargo.toml`. First builds compile the
ledger crate and its dependencies, which takes a few minutes.

## Running tests

```sh
cargo test
```

Unit tests live next to the code they exercise. Integration tests
that exercise the CLI binary live under `tests/`.

## Formatting and linting

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

Both run in CI; PRs failing either won't merge.

## Coding conventions

- Apache-2.0 license header on every new source file. See an existing
  file in `src/` for the canonical wording.
- Error handling via `anyhow::Result` at the binary boundary; library
  modules return precise errors.
- No `unwrap()` outside of tests. Prefer `?` with a clear context.
- No `unsafe`. The tool reads files and serializes; there's no excuse.

## Submitting changes

1. Fork the repository on GitHub.
2. Create a branch prefixed with a short moniker (e.g. `jill-add-flag`).
3. Write code with corresponding tests.
4. Run `cargo fmt`, `cargo clippy`, `cargo test` locally.
5. Commit with a clear message describing the change and the why.
6. Open a PR against `main`. Do **not** `--force` push during review;
   reviewers will need to re-review all commits if you do.

## License

By contributing you agree your contribution will be licensed under
Apache-2.0.
