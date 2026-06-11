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

### Known limitations

- `bytes_written` is estimated, not exact. A planned follow-up will
  construct a real `ContractDeploy` via `midnight-ledger-v9` and call
  `tree_copy` for exact numbers.
- Maintenance authority size is rolled into the wrapping constant; not
  separately checked against `AuthorityMetadataTooLarge`.
