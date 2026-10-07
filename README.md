# deploy-check

Checks whether a compiled Midnight Compact contract can be deployed in a single transaction.

A deploy writes every circuit's verifier key to the ledger, so contracts with many circuits run into limits. The one a deploy usually hits first is not the 50,000-byte block budget but the node's **per-transaction weight limit**, which caps a single transaction at about **32,000 bytes written** on every current network. A deploy between those two numbers fits the block budget and is still rejected at submission with `Transaction would exhaust the block limits`. deploy-check 0.1 checked only the block budget and passed such deploys; this version checks both.

## The ceilings

| Ceiling | Value (node 2.1.0-beta.1 presets) | Where enforced |
|---|---|---|
| Per-transaction weight | about 64% of each block dimension: **31,997** of 50,000 `bytes_written` | `frame_system::CheckWeight` (`max_extrinsic`), node weight from `get_tx_weight` |
| Block `bytes_written` budget | 50,000 bytes | ledger `TransactionLimits.block_limits` |
| Transaction byte limit | 1 MiB | ledger well-formedness |
| Per-entry-point metadata cap | 10 MiB where the preset sets it; ledger 9 only | `check_entry_point_metadata_sizes` |

### How the per-transaction ceiling is derived

The node weighs a Midnight transaction as its largest normalized cost dimension times the block weight, plus a flat per-transaction weight (`get_tx_weight`, `pallets/midnight/src/lib.rs:635-640`; `scale_normalized_cost`, `ledger/src/versions/common/mod.rs:1367`). Substrate rejects any normal-class transaction heavier than `max_extrinsic`, which `BlockWeights::with_sensible_defaults` derives as the normal share of the block, less the block-initialization allowance, less the base extrinsic weight (`runtime/src/lib.rs:307-318`). All references are to midnight-node `e01e41f8` (node-2.1.0-beta.1).

With the node's parameters:

- block weight 2 s; normal dispatch ratio 75%; block initialization 10%; base extrinsic 108,157 ns, so `max_extrinsic` = 1.5 s − 0.2 s − 108,157 ns = 1,299.892 ms;
- flat per-transaction weight 20 ms (`EXTRA_WEIGHT_TX_SIZE`, changeable by governance through `midnight.setTxSizeWeight`);
- so a transaction may use (1,299.892 − 20) ÷ 2,000 ≈ 63.99% of any block dimension, which is 31,997 of the 50,000 `bytes_written`.

Real deployments land on either side of it exactly. These figures are from a team that reported their charged `bytes_written`:

| Deployment | bytes_written | Weight | Result on chain | deploy-check 0.1 | This version |
|---|---:|---:|---|---|---|
| 9 circuits | 27,978 | 1,139 ms | confirmed | PASS | PASS |
| 12 circuits | 35,143 | 1,426 ms | rejected | PASS (wrong) | FAIL |
| 14 circuits | 38,168 | 1,547 ms | rejected | PASS (wrong) | FAIL |

A deploy too large for one transaction can still be deployed: deploy a subset of its circuits, then add the remaining verifier keys with maintenance transactions (`insertVerifierKey`).

## Limits are data, not code

deploy-check is not tied to one ledger version. Limits come from:

- **Network presets** (`--network`, default `mainnet`). `presets/networks.json` copies the `limits` object of each `res/<network>/ledger-parameters-config.json` from midnight-node verbatim, plus the weight parameters above. Run `deploy-check --list-networks` to see them. Every current preset has the same values: `block_usage` 1,000,000; `bytes_written` 50,000; `bytes_churned` 50,000,000; read and compute 2 s; transaction limit 1 MiB. To refresh for a new node release, regenerate the file from that release's `res/` directory.
- **A limits file** (`--limits-file FILE`), which overrides `--network`. It may be a node `ledger-parameters-config.json` from any node release, used unchanged; a bare node `limits` object; or the deploy-check 0.1 flat format. A top-level `weight` object, in the shape of the one in `presets/networks.json`, overrides the weight parameters.
- **Flags** for each weight parameter, applied last: `--block-weight-ns`, `--normal-dispatch-percent`, `--block-init-percent`, `--base-extrinsic-ns`, `--flat-tx-weight-ns`.

Do not use `INITIAL_LIMITS` from the ledger crate as a network's limits. It is a library default that no network runs; it understates `block_usage` 5x, `bytes_churned` 50x and the read and compute budgets 2x.

## Install

```sh
# Estimator only. Fast build, no ledger crates.
cargo install --git https://github.com/verifiably-human/deploy-check

# Exact mode for ledger 8 (pinned to ledger-8.1.0). `--features exact` is an alias.
cargo install --git https://github.com/verifiably-human/deploy-check --features exact-ledger8

# Exact mode for ledger 9 (pinned to ledger-9.1.0.0-rc.4, commit cd652d7f).
cargo install --git https://github.com/verifiably-human/deploy-check --features exact-ledger9

# Both; choose at run time with --ledger 8 or --ledger 9.
cargo install --git https://github.com/verifiably-human/deploy-check --features exact-ledger8,exact-ledger9
```

Exact builds link the ledger crates and take several minutes from a cold cache. All builds need rustc 1.95 or newer.

## Usage

```sh
deploy-check <build-dir>                          # estimator, mainnet preset
deploy-check <build-dir> --exact                  # exact bytes_written from the linked ledger
deploy-check <build-dir> --exact --ledger 9       # pick a backend when several are compiled in
deploy-check <build-dir> --exact --ctor-args '[42, "0xabcd"]'   # constructor arguments
deploy-check <build-dir> --exact --initial-state state.bin       # initial state you serialized yourself
deploy-check <build-dir> --network preprod        # another preset
deploy-check <build-dir> --limits-file ledger-parameters-config.json
deploy-check <build-dir> --bytes-written 35143    # check a figure you already have
deploy-check --list-networks
```

`<build-dir>` is the directory `compact compile` produced. It must contain `compiler/contract-info.json` and `keys/<circuit>.verifier` for every circuit, so compile without `--skip-zk`.

Other flags: `--json` emits a JSON report. For `--exact`: `--authority-keys <n>` sets the maintenance-authority committee size (default 1, what Midnight.js `deployContract` installs; 0 for none), and `--fee-allowance <bytes>` adds the wallet's fee payment (default 128, a DUST spend on ledger 9).

`--bytes-written` checks a `bytes_written` you obtained elsewhere, for example from the ledger's `Transaction.cost()` or a chain explorer. It takes precedence over exact and estimated values.

Exit codes: `0` fits every ceiling; `1` at least one ceiling exceeded; any other non-zero value is a read or parse error.

## Example

```
$ deploy-check ./build --bytes-written 35143
...
Totals: VK sum = 7715 bytes; bytes_written = 35143 bytes (supplied with --bytes-written)
Limits: network preset `mainnet` (midnight-node node-2.1.0-beta.1)

[FAIL] per-transaction weight: 35143 / 31997  (bytes_written binds; weight 1425.720 ms OVER 1299.892 ms max_extrinsic by 3146 bytes_written; the node rejects this with "Transaction would exhaust the block limits")
[PASS] block bytes_written:    35143 / 50000  (35143 bytes used, 14857 headroom)
[PASS] transaction size:       14187 / 1048576  (14187 bytes used, 1034389 headroom)
[SKIP] per-entry-point cap:    not set by these limits (ledger 8 has no such cap; for a ledger 9 network, use a limits file that sets max_contract_metadata_size)

Result: DOES NOT FIT
```

## Modes

### Estimator (default)

Adds up the verifier-key file sizes, a per-circuit allowance for the operations-map entry, a per-ledger-field allowance for initial data (from `contract-info.json`), fixed overheads and the fee payment. It needs nothing but the build directory.

It is calibrated to stay above the real charge: on Compact 0.34 builds of 1 to 18 circuits it read 12% to 47% high (36,662 bytes for an 18-circuit build the chain charged 33,068). It cannot see what a constructor writes, so a constructor that fills collections or large values can exceed it. Near the ceiling, use `--exact`.

### Exact (`--features exact-ledger8` / `exact-ledger9`)

Builds the deploy-only transaction a wallet would submit and asks the ledger for its cost (`Transaction::cost`), then adds the fee allowance. The contract state is the build's real initial state, meaning the constructor's ledger data plus one operation per circuit, with each circuit's verifier key added.

That initial state can only be produced by running the compiled contract. deploy-check runs `scripts/initial-state.mjs` (embedded in the binary) with `node`, which resolves `@midnight-ntwrk/compact-runtime` from the build's own project so its version matches the build. Pass constructor arguments with `--ctor-args`. If the constructor calls witnesses, or Node isn't available, serialize `initialState(...).currentContractState` yourself and pass it with `--initial-state`.

Each ledger version has its own backend because the crates change shape between majors. Ledger 9's `ContractOperation` has separate slots for ZKIR v2 keys (`verifier-key[v6]`, compactc's default output) and ZKIR v3 keys (`verifier-key[v7]`, from `--feature-zkir-v3`). The ledger 9 backend puts each key in the slot its tag selects. The ledger 8 backend accepts only v6 keys, since ZKIR v3 does not exist on ledger 8.

### Checked against a chain

On a devnet running node 2.1.0-beta.1 (ledger 9.1.0.0-rc.4), with no maintenance authority, as deployed:

| Build | Exact (with fee allowance) | Charged on chain | Chain's result | deploy-check |
|---|---:|---:|---|---|
| 17 circuits, 145-byte constructor argument | 31,971 | 31,971 | accepted | FITS |
| 17 circuits, 160-byte constructor argument | 32,037 | 32,037 | rejected | DOES NOT FIT |
| 18 circuits | 33,068 | 33,068 | rejected | DOES NOT FIT |

Without the fee allowance, exact mode equals the figure the ledger's own JavaScript bindings report for the same transaction: `ledger-v9` for ledger 9 builds and `ledger-v8` 8.1.0 for ledger 8 builds. The test suite checks both.

Earlier versions costed only the operations, with no initial ledger data. They undercounted by 1.6 to 2.1 KB on 17- and 18-circuit builds, and passed the 18-circuit deploy the chain rejected.

### What exact mode does not include

- Contract IR. Ledger 9 can store IR per entry point, but it is installed with a separate `IrInsert` maintenance update, not by the deploy.
- Read and compute time. A deploy runs no proof and little VM code, so `bytes_written`, or for very large transactions `block_usage`, is what binds.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
