# Changelog

All notable changes to this project will be documented here. Format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial MVP. Reads a compactc build directory, sums verifier-key
  sizes, applies a rough estimator for storage-tree overhead, and
  reports pass/fail against the three currently-enforced ledger
  ceilings (block `bytes_written`, transaction byte limit,
  per-entry-point metadata cap).
- `--network` preset flag and `--limits-file` override.
- Human-readable and JSON (`--json`) output modes.
- **Exact mode** (`--features exact` + `--exact` runtime flag). Links
  the `midnight-ledger` workspace (pinned to `ledger-8.1.0`),
  constructs a `ContractState` from the build dir's verifier keys, and
  calls `cost_model.tree_copy(Sp::new(state))` to get the precise
  `bytes_written` the ledger charges against the per-block budget.
  Validated against pattern1 (1 circuit) and pattern7-shielded-token
  (5 circuits); estimator was within 6–35% conservatively above exact.
- `.cargo/config.toml` with `net.git-fetch-with-cli = true` so the
  ledger git dep resolves on hosts where libgit2's bundled TLS fails
  the handshake (corporate proxies, some macOS configs).

### Changed

- `rust-version` bumped from `1.88` to `1.95`. Required by
  midnight-ledger's transitive `sysinfo ^0.39.1` dep. The
  estimator-only build also requires `1.95` so the published MSRV
  matches what cargo will actually accept on the locked toolchain.

### Known limitations

- Maintenance authority size is rolled into the wrapping constant; not
  separately checked against `AuthorityMetadataTooLarge`.
- The `map_insert` cost component of a deploy (a small per-entry
  overhead the ledger adds on top of `tree_copy`) is not yet included
  in the exact-mode total. Effect: exact mode under-reports by a few
  hundred bytes. Will be tracked in a follow-up.
