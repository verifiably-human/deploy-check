# SPEC-3: Exact-mode completeness

## Problem

Exact mode (`--features exact`) calls `cost_model.tree_copy(Sp::new(state.clone()))` and reports the resulting `bytes_written`. That's the on-chain storage cost — but two pieces are still being approximated:

1. **`map_insert` cost.** The ledger's deploy cost branch is:
   ```rust
   model.map_insert(EXPECTED_CONTRACT_DEPTH, false) + model.tree_copy(Sp::new(deploy.initial_state.clone()))
   ```
   We only compute the second term. The first adds a small but real number to `bytes_written` (per the ledger's cost_model — a few hundred bytes to record the new contract entry in the contracts registry map).

2. **Transaction byte limit check uses estimator constants.** Even in `--exact` mode, `estimated_tx_bytes = estimated_bytes_written + TRANSACTION_OVERHEAD` where `TRANSACTION_OVERHEAD = 4096`. We have `tagged_serialize` available when the ledger crate is linked — should serialize the real `ContractDeploy` (wrapped in whatever transaction envelope the ledger expects) and report the actual byte count.

3. **No headroom warnings.** Pass/fail is binary. A contract using 49,800 / 50,000 bytes "passes" but has no room for the smallest future change. Worth a warning.

## Goal

Three changes, all under `--features exact`:

1. Include `map_insert` cost in the exact `bytes_written` total.
2. Replace the `estimated_tx_bytes` estimator with a `tagged_serialize`-based byte count of the real `ContractDeploy`.
3. Emit `level: warning` (not fail) for ceilings used > 90% or with headroom < 5 KiB.

Estimator mode behavior is unchanged.

## Non-goals

- Computing exact cost for non-deploy actions (calls, maintenance updates). Out of scope; deploy-check is deploy-focused.
- Modeling the full transaction (zswap offers, signatures, fees). The deploy is one `ContractAction` inside a `Transaction`; we measure the deploy alone. Documented in README.

## Implementation sketch

### 1. `map_insert` cost

In `src/exact.rs::compute`:

```rust
use midnight_onchain_vm::cost_model::INITIAL_COST_MODEL;

// New: import the constant the ledger uses at the call site.
//   ledger/src/structure.rs uses `EXPECTED_CONTRACT_DEPTH`
// If it's not re-exported, define it locally with a doc comment citing the
// upstream definition.
use midnight_ledger::structure::EXPECTED_CONTRACT_DEPTH;

let tree_cost = INITIAL_COST_MODEL.tree_copy(Sp::new(state.clone()));
let registry_insert_cost = INITIAL_COST_MODEL.map_insert(EXPECTED_CONTRACT_DEPTH, false);

Ok(ExactCost {
    bytes_written: tree_cost.bytes_written + registry_insert_cost.bytes_written,
    tree_copy_bytes_written: tree_cost.bytes_written,
    map_insert_bytes_written: registry_insert_cost.bytes_written,
    /* ... */
})
```

`ExactCost` gains two breakdown fields so the report can show "X (tree_copy) + Y (map_insert) = Z" rather than a single opaque number.

Fallback: if `EXPECTED_CONTRACT_DEPTH` isn't reachable (re-export gap), define it locally with `const EXPECTED_CONTRACT_DEPTH: usize = <value>;` and a `// SOURCE: ledger/src/structure.rs:<line>` comment. The source-investigator can confirm the value.

### 2. Real transaction-byte size

```rust
use midnight_ledger::structure::ContractDeploy;
use midnight_serialize::tagged_serialize;
use rand::thread_rng;        // existing dev-dep; add as feature-gated dep here

let deploy = ContractDeploy::new(&mut thread_rng(), state.clone());
let mut buf = Vec::new();
tagged_serialize(&deploy, &mut buf)?;
let deploy_serialized_bytes = buf.len() as u64;
```

`ExactCost` gains `deploy_serialized_bytes: u64`. The transaction-byte-limit check uses this when present.

Note: serializing a real `ContractDeploy` requires a `Rng` for the nonce field. The nonce affects serialized size by exactly 32 bytes regardless of value, so any RNG suffices.

### 3. Headroom warnings

In `src/report.rs::check`, add a `headroom_threshold` parameter (default: 5 KiB or 10 % of the limit, whichever is larger). When `used + headroom_threshold > limit && used <= limit`, return a new `CheckResult` variant:

```rust
pub enum CheckOutcome { Pass, Warn, Fail }

pub struct CheckResult {
    pub outcome: CheckOutcome,
    pub used: u64,
    pub limit: u64,
    pub headroom_threshold: u64,
    pub message: String,
}
```

`Report::fits()` returns `true` for both `Pass` and `Warn` (exit 0). The CLI exits with the original behavior; a future `--strict` flag could promote warnings to failures.

## Report format changes

Human-readable output:

```
Totals: VK sum = 10595 bytes; exact bytes_written = 12491 (= 12175 tree_copy + 316 map_insert) (estimator said 12939)
        exact transaction size = 14872 bytes (estimator said 17035)

[PASS] block bytes_written:   12491 / 50000  (37509 headroom)
[PASS] transaction size:      14872 / 1048576  (1033704 headroom)
[PASS] per-entry-point cap:   2319 / 10485760  (10483441 headroom)
```

When a warning fires:

```
[WARN] block bytes_written:   47200 / 50000  (2800 headroom — under 5 KiB threshold)
```

JSON output gains:
- `totals.exact_tree_copy_bytes_written: u64` (when exact)
- `totals.exact_map_insert_bytes_written: u64` (when exact)
- `totals.exact_deploy_serialized_bytes: u64` (when exact)
- `checks.*.outcome: "pass" | "warn" | "fail"` (replacing the boolean `fits`)
- `checks.*.headroom_threshold: u64`

Backwards compatibility: keep `checks.*.fits: boolean` for one release as a derived alias of `outcome != "fail"`. Mark as deprecated in CHANGELOG. Remove in v0.3.

## Test plan

Unit:
- `compute()` returns `bytes_written == tree_copy + map_insert` when both are non-zero.
- `tagged_serialize` round-trips a real `ContractDeploy` and produces a stable byte count.
- Headroom warnings fire at exactly the threshold boundary.

Integration:
- pattern7-shielded-token current exact: 12,175 → should rise by ~few hundred bytes after map_insert is added. New value matches what `cargo test` against the ledger crate's own deploy test produces (cross-validation).
- Synthetic fixture with 200 circuits forcing a warn (47 KiB / 50 KiB).
- Synthetic fixture pushing over 50 KiB to confirm fail still fires.

## Open questions

1. Should the `Rng` for `ContractDeploy::new` be deterministic (seeded) so byte counts are reproducible across runs? Argument for: stable CI baselines. Against: the nonce is 32 bytes regardless, so non-determinism doesn't affect the count.

   **Decision (proposed):** use `rand::thread_rng()` — simple, and the resulting byte count is deterministic anyway because the only random field is exactly 32 bytes.

2. Is `EXPECTED_CONTRACT_DEPTH` re-exported from `midnight_ledger`? If not, we either fork the const or open an upstream PR to export it.

   **Decision (proposed):** check first via source-investigator. If absent, ship with a local `const` + a comment and file an upstream issue.

3. Should warnings produce non-zero exit codes when `--strict` is set? Argument for: lets CI fail-fast on headroom-cliff contracts. Against: scope creep.

   **Decision (proposed):** ship without `--strict` in this spec; add later if there's demand.
