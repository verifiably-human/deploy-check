# Changelog

All notable changes to this project will be documented here. Format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed — exact mode and the estimator no longer pass deploys the chain rejects

- **Exact mode now costs the whole deploy transaction with the ledger.** It builds the deploy-only transaction a wallet submits and takes `bytes_written` from the ledger's `Transaction::cost`, using the build's real initial state (constructor ledger data plus operations) instead of a state with null data. The old figure undercounted by 1.6 to 2.1 KB on 17- and 18-circuit builds and passed an 18-circuit deploy that the chain charged 33,068 bytes and rejected. On a ledger 9 devnet the new figure plus the fee allowance equals the chain's charge exactly (31,971 accepted; 32,037 and 33,068 rejected).
- **New `scripts/initial-state.mjs`**, embedded in the binary and run with `node`. It writes the build's initial `ContractState` using the Compact runtime from the build's own project, and falls back to a temporary `.mjs` copy when the project isn't declared an ES module. New flags `--ctor-args` and `--initial-state`.
- **New `--authority-keys`** (default 1, as Midnight.js `deployContract` installs) and **`--fee-allowance`** (default 128 bytes, the DUST fee spend measured on ledger 9).
- **Both backends rebuilt the same way:** ledger 8 (`ledger-8.1.0`) and ledger 9 (`ledger-9.1.0.0-rc.4`, which now also links `midnight-ledger-v9`). Tests compare each against the ledger's JavaScript bindings using checked-in one-circuit fixtures.
- **Estimator recalibrated to stay above the real charge.** It undercounted too (30,126 bytes for the 18-circuit build), so the earlier claim that an estimator PASS implies an exact PASS was false. It now adds a per-ledger-field allowance (read from `contract-info.json`), a larger per-circuit allowance and the fee payment. It reads 12% to 47% high on 1- to 18-circuit builds. A constructor that writes large values can still exceed it, as the README says.


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
- **Per-transaction weight ceiling.** Checks the node's per-transaction weight limit (`frame_system::CheckWeight` against the normal class's `max_extrinsic`), which is what rejects oversized deploys with "Transaction would exhaust the block limits". With node 2.1.0-beta.1 parameters it caps a transaction at 31,997 of the 50,000 `bytes_written`. Previously only the block budget was checked, so deploys between about 32,000 and 50,000 bytes passed here and failed on chain: a reported 12-circuit deploy at 35,143 bytes and a 14-circuit deploy at 38,168 bytes both passed, and both were rejected. Both now fail; a 9-circuit deploy at 27,978 bytes still passes.
- Weight parameters (block weight, normal dispatch ratio, block-initialization share, base extrinsic weight, flat per-transaction weight) are data, overridable in a limits file (`weight` object) and by `--block-weight-ns`, `--normal-dispatch-percent`, `--block-init-percent`, `--base-extrinsic-ns` and `--flat-tx-weight-ns`.
- `--bytes-written BYTES` checks a figure obtained elsewhere, such as the chain's or the ledger's `Transaction.cost()`.
- `--list-networks`.
- **Per-ledger exact backends.** `--features exact-ledger8` (pinned to `ledger-8.1.0`) and `--features exact-ledger9` (pinned to `ledger-9.1.0.0-rc.4`, commit `cd652d7f`), selectable at run time with `--ledger 8|9`. The ledger 9 backend places ZKIR v2 keys (`verifier-key[v6]`) and ZKIR v3 keys (`verifier-key[v7]`) in their respective `ContractOperation` slots. `--features exact` remains an alias for `exact-ledger8`.

### Changed

- `rust-version` bumped from `1.88` to `1.95`. Required by
  midnight-ledger's transitive `sysinfo ^0.39.1` dep. The
  estimator-only build also requires `1.95` so the published MSRV
  matches what cargo will actually accept on the locked toolchain.
- **Limits are no longer hardcoded to `INITIAL_LIMITS`.** That is a ledger library default no network runs. Network presets now live in `presets/networks.json`, which copies the `limits` object of each midnight-node `res/<network>/ledger-parameters-config.json` (node-2.1.0-beta.1) verbatim: `block_usage` 1,000,000, `bytes_written` 50,000, `bytes_churned` 50,000,000, read and compute 2 s. All ten node presets are available.
- `--limits-file` now accepts a node `ledger-parameters-config.json` from any node release unchanged, or a bare node `limits` object, as well as the 0.1 flat format.
- The per-entry-point metadata cap is checked only when the limits set `max_contract_metadata_size` (ledger 9); it is reported as skipped otherwise. Ledger 8 has no such cap.
- Exact mode now includes the operations-map insert the ledger charges on deploy (`map_insert(EXPECTED_CONTRACT_DEPTH, false)`, net +32 bytes `bytes_written`); the estimator includes it too.

### Known limitations

- Maintenance authority size is rolled into the wrapping constant; not
  separately checked against `AuthorityMetadataTooLarge`.
- Exact mode charges the contract's own state, not the rest of the deploy transaction (for example fee payment), so the on-chain figure is somewhat higher. Pass the chain's figure with `--bytes-written` when you have it.
- The per-transaction check models `bytes_written` and `block_usage` only. Read and compute time are not modeled; a deploy runs no proof and little VM code.
- The `[patch.crates-io]` entries required by the ledger 9 backend (copied from the ledger 9 workspace) apply to every build.
