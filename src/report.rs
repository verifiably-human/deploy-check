// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0

//! Per-ceiling pass/fail evaluation and reporting.
//!
//! Ceilings, in the order a deploy usually meets them:
//!
//! 1. Per-transaction weight -- dominant for deploys. A transaction's
//!    weight is its largest normalized cost dimension times the block
//!    weight, plus a flat per-transaction weight; anything over the normal
//!    class's `max_extrinsic` is rejected with "Transaction would exhaust
//!    the block limits". With node 2.1.0-beta.1 parameters that caps a
//!    transaction at about 64% of each block dimension: 31,997 of the
//!    50,000 `bytes_written`. See `limits::WeightParams`.
//! 2. Block `bytes_written` budget (50,000 bytes on every network preset).
//!    Necessary but not sufficient: a deploy between ~32,000 and 50,000
//!    bytes passes this and still fails (1).
//! 3. Transaction byte limit (1 MiB).
//! 4. Per-entry-point metadata cap (ledger 9+; skipped when unset).

use crate::artifacts::Artifacts;
use crate::limits::Limits;
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Serialize)]
pub struct Report {
    pub network_limits: Limits,
    pub circuits: Vec<CircuitReport>,
    pub totals: Totals,
    pub checks: Checks,
}

#[derive(Debug, Serialize)]
pub struct CircuitReport {
    pub name: String,
    pub verifier_key_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Totals {
    pub circuit_count: usize,
    /// Sum of verifier-key file sizes. Lower bound on bytes_written.
    pub verifier_key_bytes_total: u64,
    /// Estimated bytes_written for the deploy, including tree pointer
    /// overhead. See `report::estimate_bytes_written`. Always present.
    pub estimated_bytes_written: u64,
    /// Exact `bytes_written` from `cost_model.tree_copy` over the real
    /// `ContractState`. Present only when run with `--exact` (and the
    /// binary was built with `--features exact`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_bytes_written: Option<u64>,
    /// Caller-supplied `bytes_written` (`--bytes-written`), e.g. a figure
    /// the chain or the ledger's `Transaction.cost()` reported. When
    /// present it takes precedence over exact and estimated values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supplied_bytes_written: Option<u64>,
    /// The `bytes_written` the checks used.
    pub bytes_written_used: u64,
    /// Where `bytes_written_used` came from.
    pub bytes_written_source: String,
    /// Estimated serialized transaction size.
    pub estimated_tx_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Checks {
    pub per_transaction_weight: WeightCheck,
    pub block_bytes_written: CheckResult,
    pub transaction_byte_limit: CheckResult,
    /// `None` when the limits set no per-entry-point metadata cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_entry_point_metadata: Option<CheckResult>,
}

/// The per-transaction weight check. `used` and `limit` are in the units of
/// the binding dimension (bytes for `bytes_written` and `block_usage`), so
/// the numbers read like the block-budget check.
#[derive(Debug, Serialize)]
pub struct WeightCheck {
    pub fits: bool,
    /// Which modeled dimension produced the highest weight.
    pub binding_dimension: &'static str,
    pub used: u64,
    /// Most of the binding dimension one transaction may use.
    pub limit: u64,
    pub tx_weight_ps: u64,
    pub max_extrinsic_ps: u64,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub fits: bool,
    pub used: u64,
    pub limit: u64,
    pub message: String,
}

impl Report {
    pub fn fits(&self) -> bool {
        self.checks.per_transaction_weight.fits
            && self.checks.block_bytes_written.fits
            && self.checks.transaction_byte_limit.fits
            && self
                .checks
                .per_entry_point_metadata
                .as_ref()
                .is_none_or(|c| c.fits)
    }

    pub fn print_human(&self, out: &mut impl Write) -> std::io::Result<()> {
        writeln!(out, "deploy-check report")?;
        writeln!(out, "===================")?;
        writeln!(out)?;
        writeln!(out, "Circuits ({}):", self.totals.circuit_count)?;
        for c in &self.circuits {
            writeln!(
                out,
                "  {:<32} {:>8} bytes (VK)",
                c.name, c.verifier_key_bytes
            )?;
        }
        writeln!(out)?;
        writeln!(
            out,
            "Totals: VK sum = {} bytes; bytes_written = {} bytes ({})",
            self.totals.verifier_key_bytes_total,
            self.totals.bytes_written_used,
            self.totals.bytes_written_source
        )?;
        if self.totals.supplied_bytes_written.is_none() && self.totals.exact_bytes_written.is_none()
        {
            writeln!(
                out,
                "        estimator figure; run with --exact (or pass --bytes-written) for the real value"
            )?;
        }
        writeln!(out, "Limits: {}", self.network_limits.label)?;
        writeln!(out)?;
        let pf = |fits: bool| if fits { "PASS" } else { "FAIL" };
        let w = &self.checks.per_transaction_weight;
        writeln!(
            out,
            "[{}] per-transaction weight: {} / {}  ({})",
            pf(w.fits),
            w.used,
            w.limit,
            w.message
        )?;
        let b = &self.checks.block_bytes_written;
        writeln!(
            out,
            "[{}] block bytes_written:    {} / {}  ({})",
            pf(b.fits),
            b.used,
            b.limit,
            b.message
        )?;
        let t = &self.checks.transaction_byte_limit;
        writeln!(
            out,
            "[{}] transaction size:       {} / {}  ({})",
            pf(t.fits),
            t.used,
            t.limit,
            t.message
        )?;
        match &self.checks.per_entry_point_metadata {
            Some(m) => writeln!(
                out,
                "[{}] per-entry-point cap:    {} / {}  ({})",
                pf(m.fits),
                m.used,
                m.limit,
                m.message
            )?,
            None => writeln!(
                out,
                "[SKIP] per-entry-point cap:    not set by these limits (ledger 8 has no such cap; for a ledger 9 network, use a limits file that sets max_contract_metadata_size)"
            )?,
        }
        writeln!(out)?;
        writeln!(
            out,
            "Result: {}",
            if self.fits() { "FITS" } else { "DOES NOT FIT" }
        )?;
        Ok(())
    }
}

/// Per-child pointer overhead in the on-chain storage tree.
/// Matches `PERSISTENT_HASH_BYTES` (32) in midnight-ledger.
const PERSISTENT_HASH_BYTES: u64 = 32;
/// Per-entry-point wrapper overhead (name + ContractOperation framing).
/// Rough; the ledger crate gives an exact figure, but for this MVP we
/// use the doc's rule-of-thumb upper bound.
const ENTRY_POINT_OVERHEAD: u64 = 200;
/// Wrapping ContractState overhead (operations-map root, balance map,
/// maintenance authority commitments, initial state ADT root).
/// Rough constant; refine when wiring the ledger crate.
const CONTRACT_STATE_OVERHEAD: u64 = 1024;
/// Wrapping transaction overhead (Intent, balance commitments,
/// zswap offers, signatures). Rough; refine.
const TRANSACTION_OVERHEAD: u64 = 4096;

pub fn evaluate(
    artifacts: &Artifacts,
    limits: &Limits,
    exact_bytes_written: Option<u64>,
    supplied_bytes_written: Option<u64>,
) -> Report {
    let circuits: Vec<CircuitReport> = artifacts
        .circuits
        .iter()
        .map(|c| CircuitReport {
            name: c.name.clone(),
            verifier_key_bytes: c.verifier_key_bytes.len() as u64,
        })
        .collect();

    let vk_sum: u64 = circuits.iter().map(|c| c.verifier_key_bytes).sum();
    let circuit_count = circuits.len();

    let estimated_bytes_written = estimate_bytes_written(circuit_count as u64, vk_sum);
    let estimated_tx_bytes = estimated_bytes_written + TRANSACTION_OVERHEAD;
    let max_ep_bytes = circuits
        .iter()
        .map(|c| c.verifier_key_bytes + ENTRY_POINT_OVERHEAD)
        .max()
        .unwrap_or(0);

    // Precedence: a caller-supplied figure, then exact, then the estimator.
    let (bytes_written_used, bytes_written_source) =
        match (supplied_bytes_written, exact_bytes_written) {
            (Some(b), _) => (b, "supplied with --bytes-written".to_string()),
            (None, Some(b)) => (
                b,
                "exact: ledger tree_copy of the initial state + operations-map insert".to_string(),
            ),
            (None, None) => (
                estimated_bytes_written,
                "estimated from VK sum + tree overhead".to_string(),
            ),
        };

    let per_transaction_weight = weight_check(limits, bytes_written_used, estimated_tx_bytes);
    let block_bytes_written = check(
        &bytes_written_source,
        bytes_written_used,
        limits.block_limits.bytes_written,
    );
    let transaction_byte_limit = check(
        "estimated serialized transaction size",
        estimated_tx_bytes,
        limits.transaction_byte_limit,
    );
    let per_entry_point_metadata = limits.max_contract_metadata_size.map(|cap| {
        check(
            "largest (entry_point, ContractOperation) pair",
            max_ep_bytes,
            cap,
        )
    });

    Report {
        network_limits: limits.clone(),
        circuits,
        totals: Totals {
            circuit_count,
            verifier_key_bytes_total: vk_sum,
            estimated_bytes_written,
            exact_bytes_written,
            supplied_bytes_written,
            bytes_written_used,
            bytes_written_source,
            estimated_tx_bytes,
        },
        checks: Checks {
            per_transaction_weight,
            block_bytes_written,
            transaction_byte_limit,
            per_entry_point_metadata,
        },
    }
}

/// Per-transaction weight over the dimensions a deploy meaningfully uses:
/// `bytes_written` (verifier keys and state) and `block_usage` (serialized
/// size). A deploy runs no proof and little VM code, so read and compute
/// time stay far below either; they are not modeled.
fn weight_check(limits: &Limits, bytes_written: u64, tx_bytes: u64) -> WeightCheck {
    let w = &limits.weight;
    let dims = [
        (
            "bytes_written",
            bytes_written,
            limits.block_limits.bytes_written,
        ),
        ("block_usage", tx_bytes, limits.block_limits.block_usage),
    ];
    let (name, used, block_limit) = dims
        .into_iter()
        .max_by_key(|(_, used, limit)| w.tx_weight_ps(*used, *limit))
        .expect("non-empty");
    let tx_weight_ps = w.tx_weight_ps(used, block_limit);
    let max_extrinsic_ps = w.max_extrinsic_ps();
    let allowance = w.per_tx_allowance(block_limit);
    let fits = tx_weight_ps <= max_extrinsic_ps;
    let ms = |ps: u64| ps as f64 / 1e9;
    let message = if fits {
        format!(
            "{name} binds; weight {:.3} ms of {:.3} ms max_extrinsic, {} {name} headroom",
            ms(tx_weight_ps),
            ms(max_extrinsic_ps),
            allowance.saturating_sub(used)
        )
    } else {
        format!(
            "{name} binds; weight {:.3} ms OVER {:.3} ms max_extrinsic by {} {name}; the node rejects this with \"Transaction would exhaust the block limits\"",
            ms(tx_weight_ps),
            ms(max_extrinsic_ps),
            used.saturating_sub(allowance)
        )
    };
    WeightCheck {
        fits,
        binding_dimension: name,
        used,
        limit: allowance,
        tx_weight_ps,
        max_extrinsic_ps,
        message,
    }
}

/// MVP estimator. Mirrors `tree_copy` accounting from `cost_model.rs`:
/// bytes_written = sum(node.data.len + child_count * PERSISTENT_HASH_BYTES).
/// We model: per circuit, one operations-map entry with a child pointer
/// plus the VK bytes. Plus the ContractState wrapper.
fn estimate_bytes_written(circuit_count: u64, vk_sum: u64) -> u64 {
    let map_node_overhead = circuit_count * PERSISTENT_HASH_BYTES * 2;
    let per_entry_wrapper = circuit_count * ENTRY_POINT_OVERHEAD;
    vk_sum
        + map_node_overhead
        + per_entry_wrapper
        + CONTRACT_STATE_OVERHEAD
        + crate::exact::DEPLOY_MAP_INSERT_NET_BYTES
}

fn check(what: &str, used: u64, limit: u64) -> CheckResult {
    let fits = used <= limit;
    CheckResult {
        fits,
        used,
        limit,
        message: if fits {
            format!(
                "{} bytes used, {} headroom",
                used,
                limit.saturating_sub(used)
            )
        } else {
            format!("{} bytes used, {} OVER ({})", used, used - limit, what)
        },
    }
}
