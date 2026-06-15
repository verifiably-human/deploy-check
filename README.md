# deploy-check

Checks whether a compiled Midnight Compact contract fits under the deploy
limits enforced by the ledger.

The Midnight ledger rejects a contract deploy when any of four ceilings
is exceeded:

| Ceiling | Value (INITIAL_LIMITS) | Where enforced |
|---|---|---|
| Block `bytes_written` budget | 50 KiB | `Transaction::fees → cost.normalize` |
| Transaction byte limit | 1 MiB | `Transaction::well_formed` |
| Per-entry-point metadata cap | 10 MiB | `check_entry_point_metadata_sizes` |
| `VerifierKeyTooLarge` | declared, not enforced | `ledger/src/error.rs` |

The dominant constraint for real contracts is the 50 KiB block
`bytes_written` budget: deploying a contract is a pure write, and its
cost is `tree_copy(initial_state)` — the serialized size of the
verifier keys, operations map, maintenance authority, and initial
state ADT, plus 32 bytes per child pointer.

`deploy-check` reads a compactc build directory, computes the deploy
footprint, and reports pass/fail per ceiling.

## Install

Two install variants. Pick based on whether you want the lightweight
estimator only, or the real `cost_model.tree_copy` numbers from the
midnight-ledger crate.

```sh
# Lightweight: estimator-only. Fast build, no ledger crate linkage.
cargo install --git https://github.com/verifiably-human/deploy-check

# Exact: links midnight-ledger workspace. First build pulls ~50 crates
# and takes several minutes; subsequent builds reuse the cache.
cargo install --git https://github.com/verifiably-human/deploy-check --features exact
```

The exact-mode build requires **rustc 1.95 or newer** (midnight-ledger's
transitive dependency on `sysinfo` enforces this). The estimator-only
build works on the same minimum.

## Usage

```sh
deploy-check <build-dir>            # estimator mode (default)
deploy-check <build-dir> --exact    # exact tree_copy mode (requires --features exact at install)
```

`<build-dir>` is the directory compactc produced (the same path you
passed as the second argument to `compactc`). It must contain:

- `compiler/contract-info.json`
- `keys/<circuit>.verifier` for every circuit declared in
  `contract-info.json`

The `keys/` directory only exists after `zkir compile-many` ran, so
you must compile without `--skip-zk` for this tool to work.

### Flags

- `--network <mainnet|preview|qanet|local>` — pick the limits preset.
  All presets currently track `INITIAL_LIMITS`; the flag exists so we
  can diverge if governance raises a limit on a network.
- `--limits-file <path>` — JSON of `TransactionLimits` overriding the
  preset. Useful for testing against custom devnets.
- `--exact` — use real `cost_model.tree_copy` from midnight-ledger
  instead of the estimator. Requires the binary to be built with
  `--features exact`; otherwise returns a clear error.
- `--format <text|json|sarif>` — output format. `text` is the default
  human-readable. `json` emits the report serialised. `sarif` emits a
  SARIF 2.1.0 log suitable for upload via
  `github/codeql-action/upload-sarif` (failed ceiling checks become
  SARIF results with `level: error`).
- `--json` — deprecated alias for `--format json`. Still works.

### Exit codes

- `0` — fits under every ceiling.
- `1` — at least one ceiling exceeded.
- non-zero (other) — read or parse error.

## Example

Estimator mode (default):

```
$ deploy-check ./pattern7-shielded-token
deploy-check report
===================

Circuits (5):
  initializeToken                      2119 bytes (VK)
  mintShieldedComplianceToken          2119 bytes (VK)
  receiveShieldedComplianceTokens      2119 bytes (VK)
  compliantShieldedTransfer            2119 bytes (VK)
  mintAndSendShielded                  2119 bytes (VK)

Totals: VK sum = 10595 bytes; estimated bytes_written = 12939 bytes (run with --exact for the real value)

[PASS] block bytes_written:   12939 / 50000  (37061 headroom)
[PASS] transaction size:      17035 / 1048576  (1031541 headroom)
[PASS] per-entry-point cap:   2319 / 10485760  (10483441 headroom)

Result: FITS
```

Exact mode (`--features exact` build + `--exact` flag):

```
$ deploy-check ./pattern7-shielded-token --exact
...
Totals: VK sum = 10595 bytes; exact bytes_written = 12175 bytes (estimator said 12939)

[PASS] block bytes_written:   12175 / 50000  (37825 headroom)
...
Result: FITS
```

The estimator is intentionally conservative (over-counts slightly), so
a `PASS` in estimator mode is a strong signal you'll also `PASS` in
exact mode. Use the estimator in tight dev loops; use exact mode in CI
deploy gates.

## Modes

### Estimator (default)

Sums verifier-key file sizes and adds a constant for tree-pointer
overhead. No ledger crate linkage. Build cost: a handful of crates,
seconds.

The constants (`PERSISTENT_HASH_BYTES`, `ENTRY_POINT_OVERHEAD`,
`CONTRACT_STATE_OVERHEAD`, `TRANSACTION_OVERHEAD`) are sized to
slightly overshoot real values so an estimator `PASS` implies an exact
`PASS`. Verified against pattern1 (35% over) and
pattern7-shielded-token (6% over).

### Exact (`--features exact`)

Links `midnight-ledger` workspace (ledger-8.1.0), constructs a real
`ContractState` from the build dir's verifier keys, and calls
`cost_model.tree_copy(Sp::new(state))`. The returned `bytes_written`
is the exact value the ledger will charge against the block budget.

First build pulls ~50 crates and takes several minutes. Subsequent
builds reuse cargo's incremental cache. Use this in CI gates, public
release smoke tests, and any time the estimator's headroom is < ~5 KiB.

## Source of truth

All limits and the cost model are pinned to
[midnight-ledger@ledger-8.1.0](https://github.com/midnightntwrk/midnight-ledger/tree/ledger-8.1.0).
Bumping the dep may change reported numbers; see `Cargo.toml`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
