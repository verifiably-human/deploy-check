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

```sh
cargo install --git https://github.com/verifiably-human/deploy-check
```

## Usage

```sh
deploy-check <build-dir>
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
- `--json` — emit a JSON report instead of human-readable.

### Exit codes

- `0` — fits under every ceiling.
- `1` — at least one ceiling exceeded.
- non-zero (other) — read or parse error.

## Example

```
$ deploy-check ./my-contract-build
deploy-check report
===================

Circuits (3):
  init                              2872 bytes (VK)
  transfer                          2880 bytes (VK)
  withdraw                          2871 bytes (VK)

Totals: VK sum = 8623 bytes; estimated bytes_written = 10247 bytes

[PASS] block bytes_written:   10247 / 50000  (39753 headroom)
[PASS] transaction size:      14343 / 1048576  (1034233 headroom)
[PASS] per-entry-point cap:   3080 / 10485760  (10482680 headroom)

Result: FITS
```

## Status

Early. The MVP uses an estimator for `bytes_written` that sums
verifier-key file sizes and adds a constant for tree pointer
overhead. A planned follow-up will construct a real `ContractDeploy`
via the `midnight-ledger-v9` crate and call `tree_copy` for exact
numbers.

## Source of truth

All limits and the cost model are pinned to a specific commit of
[midnight-ledger](https://github.com/midnightntwrk/midnight-ledger).
Bumping the dep may change the reported numbers; see `Cargo.toml`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
