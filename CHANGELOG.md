# Changelog

All notable changes to this project will be documented here. Format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added — Exact-mode completeness (see specs/SPEC-3-exact-mode-completeness.md)

- **`map_insert` cost now included in exact mode.** The ledger's
  deploy cost is `map_insert + tree_copy`; previously only `tree_copy`
  was computed. Pattern7-shielded-token: exact bytes_written went from
  12,175 → 16,303 (the missing 4,128 bytes is the registry insert).
  `map_insert` is `pub(crate)` upstream, so the arithmetic is
  replicated locally with a `SOURCE:` comment pointing at the upstream
  function for re-verification when the dep is bumped.
- **Real `ContractDeploy` serialization.** In `--exact` mode the
  transaction-byte-limit check now uses
  `tagged_serialize(ContractDeploy)` instead of the estimator constant.
  Pattern1: real tx size 2,257 bytes vs estimator 7,503 (the deploy
  envelope is much smaller than the rough constant assumed).
- **Headroom warnings.** A passing check whose headroom is under
  `max(5 KiB, 10% of limit)` now emits a `WARN` outcome (still
  exit 0). Helps catch contracts that fit today but won't survive
  minor growth.
- **`CheckOutcome` enum (`pass | warn | fail`)** added to JSON output
  alongside the deprecated `fits` boolean. CI scripts that want the
  three-way verdict should read `outcome`; scripts using `fits`
  continue to work for one release.
- **`headroom_threshold_bytes`** exposed per check in JSON.
- **Exact-mode breakdown** in human + JSON output:
  `exact bytes_written = X (= Y tree_copy + Z map_insert)`.
- **2 new integration tests:** `json_includes_outcome_field`,
  `warns_when_headroom_under_threshold`.

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
