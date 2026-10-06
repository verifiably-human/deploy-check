// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0

//! Deploy-time ceilings, as data.
//!
//! Nothing here is tied to one ledger version. Limits come from one of:
//!
//! - an embedded network preset (`presets/networks.json`), which copies the
//!   `limits` object of each `res/<network>/ledger-parameters-config.json`
//!   in midnight-node verbatim;
//! - a `--limits-file`, which may be a node `ledger-parameters-config.json`
//!   as shipped by any node release, a bare node `limits` object, or the
//!   legacy deploy-check 0.1 flat format.
//!
//! The Substrate weight parameters that produce the per-transaction ceiling
//! are kept alongside, because a deploy that fits the block budget can still
//! be rejected for exhausting the per-transaction weight. See
//! [`WeightParams`].
//!
//! Do NOT substitute `INITIAL_LIMITS` from `ledger/src/structure.rs`: it is a
//! library default that no network runs (it understates `block_usage` 5x,
//! `bytes_churned` 50x and the read/compute budgets 2x).

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Embedded presets. Regenerate from a node checkout when a release changes
/// `res/*/ledger-parameters-config.json`.
const PRESETS_JSON: &str = include_str!("../presets/networks.json");

/// Default network when neither `--network` nor `--limits-file` is given.
pub const DEFAULT_NETWORK: &str = "mainnet";

/// The ledger's per-block budgets (`TransactionLimits.block_limits`), in the
/// ledger's own units: picoseconds for time, bytes for the rest.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockLimits {
    pub read_time_ps: u64,
    pub compute_time_ps: u64,
    pub block_usage: u64,
    pub bytes_written: u64,
    pub bytes_churned: u64,
}

/// Substrate weight parameters that bound a single transaction.
///
/// The node weighs a Midnight transaction as
/// `max(normalized cost dimensions) * block_weight + flat_tx_weight`
/// (`get_tx_weight`, pallets/midnight/src/lib.rs:635-640;
/// `scale_normalized_cost`, ledger/src/versions/common/mod.rs:1367, both at
/// midnight-node e01e41f8). `frame_system::CheckWeight` rejects any
/// transaction heavier than the normal class's `max_extrinsic` with
/// `InvalidTransaction::ExhaustsResources` ("Transaction would exhaust the
/// block limits"). `max_extrinsic` is derived by
/// `BlockWeights::with_sensible_defaults` (runtime/src/lib.rs:307-318) as
/// the normal-class share of the block, less the block-initialization
/// allowance, less the base extrinsic weight.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeightParams {
    /// `BlockWeights` max block ref_time, picoseconds. node 2.1.0-beta.1: 2 s.
    pub block_weight_ps: u64,
    /// Normal dispatch ratio, percent. node 2.1.0-beta.1: 75.
    pub normal_dispatch_percent: u32,
    /// `avg_block_initialization`, percent of the block. Substrate's
    /// `with_sensible_defaults`: 10.
    pub block_init_percent: u32,
    /// `ExtrinsicBaseWeight`, picoseconds. polkadot-sdk: 108,157 ns.
    pub base_extrinsic_ps: u64,
    /// `ConfigurableTransactionSizeWeight`, picoseconds. Defaults to
    /// `EXTRA_WEIGHT_TX_SIZE` (20 ms); governance can change it with
    /// `midnight.setTxSizeWeight`, so check the live value for a network
    /// whose governance has acted.
    pub flat_tx_weight_ps: u64,
}

impl WeightParams {
    /// Largest weight a single normal-class transaction may carry.
    pub fn max_extrinsic_ps(&self) -> u64 {
        let block = self.block_weight_ps as u128;
        let normal = block * self.normal_dispatch_percent as u128 / 100;
        let init = block * self.block_init_percent as u128 / 100;
        normal
            .saturating_sub(init)
            .saturating_sub(self.base_extrinsic_ps as u128) as u64
    }

    /// Weight of a transaction whose largest normalized cost dimension is
    /// `used / limit`.
    pub fn tx_weight_ps(&self, used: u64, limit: u64) -> u64 {
        if limit == 0 {
            return u64::MAX;
        }
        let scaled = (used as u128 * self.block_weight_ps as u128 / limit as u128)
            .min(self.block_weight_ps as u128);
        (scaled + self.flat_tx_weight_ps as u128).min(u64::MAX as u128) as u64
    }

    /// The most of a dimension with block limit `limit` that one
    /// transaction may use before its weight exceeds `max_extrinsic`.
    /// Rounds down. The ledger's fixed-point scaling can differ from this
    /// by about one unit at the boundary.
    pub fn per_tx_allowance(&self, limit: u64) -> u64 {
        let headroom = self
            .max_extrinsic_ps()
            .saturating_sub(self.flat_tx_weight_ps) as u128;
        (limit as u128 * headroom / self.block_weight_ps.max(1) as u128) as u64
    }
}

/// Everything the checks need, plus where it came from.
#[derive(Debug, Clone, Serialize)]
pub struct Limits {
    /// Short label for human-readable output.
    pub label: String,
    /// Full provenance of these numbers.
    pub source: String,
    /// Maximum serialized transaction size, bytes.
    pub transaction_byte_limit: u64,
    /// Per-entry-point metadata cap (ledger 9+). `None` when the preset or
    /// file does not set it; ledger 8 has no such cap.
    pub max_contract_metadata_size: Option<u64>,
    pub block_limits: BlockLimits,
    pub weight: WeightParams,
}

#[derive(Debug, Deserialize)]
struct PresetFile {
    source: String,
    weight: WeightParams,
    networks: serde_json::Map<String, Value>,
}

fn presets() -> Result<PresetFile> {
    serde_json::from_str(PRESETS_JSON).context("parsing embedded presets/networks.json")
}

/// Names of the embedded network presets.
pub fn network_names() -> Vec<String> {
    presets()
        .map(|p| p.networks.keys().cloned().collect())
        .unwrap_or_default()
}

/// Limits for a named network preset.
pub fn for_network(name: &str) -> Result<Limits> {
    let p = presets()?;
    let entry = p.networks.get(name).ok_or_else(|| {
        anyhow!(
            "unknown network `{name}`; available presets: {}",
            p.networks.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })?;
    let limits = entry
        .get("limits")
        .ok_or_else(|| anyhow!("preset `{name}` has no `limits` object"))?;
    from_node_limits(
        limits,
        p.weight,
        format!("network preset `{name}` (midnight-node node-2.1.0-beta.1)"),
        format!("network preset `{name}`: {}", p.source),
    )
}

/// Default weight parameters from the embedded presets.
pub fn default_weight() -> Result<WeightParams> {
    Ok(presets()?.weight)
}

/// Parse a node `limits` object (the `limits` key of
/// `ledger-parameters-config.json`).
fn from_node_limits(
    v: &Value,
    weight: WeightParams,
    label: String,
    source: String,
) -> Result<Limits> {
    let bl = v
        .get("block_limits")
        .ok_or_else(|| anyhow!("`limits` object has no `block_limits`"))?;
    let num = |obj: &Value, key: &str| -> Result<u64> {
        obj.get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("missing or non-integer `{key}`"))
    };
    Ok(Limits {
        label,
        source,
        transaction_byte_limit: num(v, "transaction_byte_limit")?,
        max_contract_metadata_size: v.get("max_contract_metadata_size").and_then(Value::as_u64),
        block_limits: BlockLimits {
            read_time_ps: num(bl, "readTime")?,
            compute_time_ps: num(bl, "computeTime")?,
            block_usage: num(bl, "blockUsage")?,
            bytes_written: num(bl, "bytesWritten")?,
            bytes_churned: num(bl, "bytesChurned")?,
        },
        weight,
    })
}

/// Load limits from a file. Accepted shapes, detected automatically:
///
/// 1. A node `ledger-parameters-config.json` (top-level `limits` object).
///    An optional top-level `weight` object overrides the weight defaults,
///    so the same shape doubles as deploy-check's own format.
/// 2. A bare node `limits` object (top-level `block_limits`).
/// 3. The legacy deploy-check 0.1 flat format (`transaction_byte_limit`,
///    `max_contract_metadata_size`, `block_bytes_written`). Block
///    dimensions it lacks are taken from the default network preset.
pub fn load_from_file(path: &Path) -> Result<Limits> {
    let text = std::fs::read_to_string(path)?;
    let v: Value = serde_json::from_str(&text).context("file is not valid JSON")?;
    let source = format!("limits file {}", path.display());

    let weight = match v.get("weight") {
        Some(w) => serde_json::from_value(w.clone()).context("parsing `weight` object")?,
        None => default_weight()?,
    };

    if let Some(limits) = v.get("limits") {
        return from_node_limits(limits, weight, source.clone(), source);
    }
    if v.get("block_limits").is_some() {
        return from_node_limits(&v, weight, source.clone(), source);
    }
    if let Some(bw) = v.get("block_bytes_written").and_then(Value::as_u64) {
        let mut l = for_network(DEFAULT_NETWORK)?;
        l.label = format!("{source} (legacy flat format)");
        l.source = format!(
            "{source} (legacy flat format; other block dimensions from `{DEFAULT_NETWORK}` preset)"
        );
        l.block_limits.bytes_written = bw;
        if let Some(t) = v.get("transaction_byte_limit").and_then(Value::as_u64) {
            l.transaction_byte_limit = t;
        }
        l.max_contract_metadata_size = v.get("max_contract_metadata_size").and_then(Value::as_u64);
        l.weight = weight;
        return Ok(l);
    }
    bail!(
        "unrecognized limits file: expected a node ledger-parameters-config.json (with `limits`), a bare `limits` object (with `block_limits`), or the legacy flat format (with `block_bytes_written`)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_defaults() -> WeightParams {
        default_weight().unwrap()
    }

    #[test]
    fn max_extrinsic_matches_substrate_derivation() {
        // 1.5 s normal - 0.2 s init - 108,157 ns base.
        assert_eq!(node_defaults().max_extrinsic_ps(), 1_299_891_843_000);
    }

    #[test]
    fn bytes_written_allowance_is_about_32k() {
        assert_eq!(node_defaults().per_tx_allowance(50_000), 31_997);
    }

    #[test]
    fn every_preset_parses() {
        for n in network_names() {
            let l = for_network(&n).unwrap();
            assert_eq!(l.block_limits.bytes_written, 50_000, "{n}");
            assert_eq!(l.block_limits.block_usage, 1_000_000, "{n}");
        }
    }
}
