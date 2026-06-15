// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0

//! Per-ceiling pass/warn/fail evaluation and reporting.
//!
//! The four ceilings from `contract-deploy-size-limits.md`:
//!
//! 1. Block `bytes_written` budget (50 KiB on mainnet) -- dominant.
//! 2. Transaction byte limit (1 MiB) -- only bites many-circuit contracts.
//! 3. Per-entry-point metadata cap (10 MiB) -- almost never the cause.
//! 4. VerifierKeyTooLarge -- declared but not enforced in current ledger.

use crate::artifacts::Artifacts;
use crate::limits::Limits;
use serde::Serialize;
use std::io::Write;

/// Breakdown of values produced by the real ledger cost model
/// (only populated when `--exact` ran successfully). All fields are
/// optional so the report stays meaningful in estimator-only mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExactSizes {
    /// `bytes_written` total: map_insert + tree_copy.
    pub bytes_written: u64,
    /// `tree_copy` portion of `bytes_written`.
    pub tree_copy_bytes_written: u64,
    /// `map_insert` portion of `bytes_written`.
    pub map_insert_bytes_written: u64,
    /// Tagged-serialized length of the `ContractDeploy`. Used as the
    /// transaction-byte-limit check value in exact mode.
    pub deploy_serialized_bytes: u64,
}

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
    /// overhead. Always present.
    pub estimated_bytes_written: u64,
    /// Exact `bytes_written` (= map_insert + tree_copy). Present only
    /// when run with `--exact`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_bytes_written: Option<u64>,
    /// `tree_copy` portion of exact bytes_written. Present only in exact mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_tree_copy_bytes_written: Option<u64>,
    /// `map_insert` portion of exact bytes_written. Present only in exact mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_map_insert_bytes_written: Option<u64>,
    /// Estimated serialized transaction size.
    pub estimated_tx_bytes: u64,
    /// Real tagged-serialized ContractDeploy size. Present only in exact mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_deploy_serialized_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Checks {
    pub block_bytes_written: CheckResult,
    pub transaction_byte_limit: CheckResult,
    pub per_entry_point_metadata: CheckResult,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CheckOutcome {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub outcome: CheckOutcome,
    /// Deprecated alias for outcome != Fail. Kept for one release as a
    /// boolean shortcut consumed by older CI scripts.
    pub fits: bool,
    pub used: u64,
    pub limit: u64,
    /// Threshold below which a passing check upgrades to a warning.
    pub headroom_threshold_bytes: u64,
    pub message: String,
}

impl Report {
    /// True when every check is Pass or Warn. CI gates that want to
    /// treat warnings as failures should look at the individual outcomes.
    pub fn fits(&self) -> bool {
        self.checks.block_bytes_written.outcome != CheckOutcome::Fail
            && self.checks.transaction_byte_limit.outcome != CheckOutcome::Fail
            && self.checks.per_entry_point_metadata.outcome != CheckOutcome::Fail
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
        if let (Some(exact), Some(tree), Some(insert)) = (
            self.totals.exact_bytes_written,
            self.totals.exact_tree_copy_bytes_written,
            self.totals.exact_map_insert_bytes_written,
        ) {
            writeln!(
                out,
                "Totals: VK sum = {} bytes; exact bytes_written = {} (= {} tree_copy + {} map_insert) (estimator said {})",
                self.totals.verifier_key_bytes_total,
                exact,
                tree,
                insert,
                self.totals.estimated_bytes_written
            )?;
            if let Some(serialized) = self.totals.exact_deploy_serialized_bytes {
                writeln!(
                    out,
                    "        exact transaction size = {} bytes (estimator said {})",
                    serialized, self.totals.estimated_tx_bytes
                )?;
            }
        } else {
            writeln!(
                out,
                "Totals: VK sum = {} bytes; estimated bytes_written = {} bytes (run with --exact for the real value)",
                self.totals.verifier_key_bytes_total, self.totals.estimated_bytes_written
            )?;
        }
        writeln!(out)?;
        let tag = |c: &CheckResult| match c.outcome {
            CheckOutcome::Pass => "PASS",
            CheckOutcome::Warn => "WARN",
            CheckOutcome::Fail => "FAIL",
        };
        writeln!(
            out,
            "[{}] block bytes_written:   {} / {}  ({})",
            tag(&self.checks.block_bytes_written),
            self.checks.block_bytes_written.used,
            self.checks.block_bytes_written.limit,
            self.checks.block_bytes_written.message
        )?;
        writeln!(
            out,
            "[{}] transaction size:      {} / {}  ({})",
            tag(&self.checks.transaction_byte_limit),
            self.checks.transaction_byte_limit.used,
            self.checks.transaction_byte_limit.limit,
            self.checks.transaction_byte_limit.message
        )?;
        writeln!(
            out,
            "[{}] per-entry-point cap:   {} / {}  ({})",
            tag(&self.checks.per_entry_point_metadata),
            self.checks.per_entry_point_metadata.used,
            self.checks.per_entry_point_metadata.limit,
            self.checks.per_entry_point_metadata.message
        )?;
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
/// Per-entry-point wrapper overhead.
const ENTRY_POINT_OVERHEAD: u64 = 200;
/// Wrapping ContractState overhead.
const CONTRACT_STATE_OVERHEAD: u64 = 1024;
/// Wrapping transaction overhead (estimator-only).
const TRANSACTION_OVERHEAD: u64 = 4096;

/// Floor for the headroom-warning threshold. A check using more than
/// `limit - max(5 KiB, 10% of limit)` bytes triggers a warning.
const HEADROOM_FLOOR_BYTES: u64 = 5 * 1024;

fn headroom_threshold(limit: u64) -> u64 {
    let ten_percent = limit / 10;
    HEADROOM_FLOOR_BYTES.max(ten_percent)
}

pub fn evaluate(
    artifacts: &Artifacts,
    limits: &Limits,
    exact: Option<ExactSizes>,
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

    let (bw_used, bw_source) = match exact {
        Some(e) => (e.bytes_written, "exact bytes_written = map_insert + tree_copy"),
        None => (estimated_bytes_written, "estimated bytes_written from VK sum + tree overhead"),
    };
    let (tx_used, tx_source) = match exact {
        Some(e) => (e.deploy_serialized_bytes, "exact serialized ContractDeploy size"),
        None => (estimated_tx_bytes, "estimated serialized transaction size"),
    };

    let block_bytes_written = check(bw_source, bw_used, limits.block_bytes_written);
    let transaction_byte_limit = check(tx_source, tx_used, limits.transaction_byte_limit);
    let per_entry_point_metadata = check(
        "largest (entry_point, ContractOperation) pair",
        max_ep_bytes,
        limits.max_contract_metadata_size,
    );

    Report {
        network_limits: *limits,
        circuits,
        totals: Totals {
            circuit_count,
            verifier_key_bytes_total: vk_sum,
            estimated_bytes_written,
            exact_bytes_written: exact.map(|e| e.bytes_written),
            exact_tree_copy_bytes_written: exact.map(|e| e.tree_copy_bytes_written),
            exact_map_insert_bytes_written: exact.map(|e| e.map_insert_bytes_written),
            estimated_tx_bytes,
            exact_deploy_serialized_bytes: exact.map(|e| e.deploy_serialized_bytes),
        },
        checks: Checks {
            block_bytes_written,
            transaction_byte_limit,
            per_entry_point_metadata,
        },
    }
}

fn estimate_bytes_written(circuit_count: u64, vk_sum: u64) -> u64 {
    let map_node_overhead = circuit_count * PERSISTENT_HASH_BYTES * 2;
    let per_entry_wrapper = circuit_count * ENTRY_POINT_OVERHEAD;
    vk_sum + map_node_overhead + per_entry_wrapper + CONTRACT_STATE_OVERHEAD
}

fn check(what: &str, used: u64, limit: u64) -> CheckResult {
    let threshold = headroom_threshold(limit);
    let outcome = if used > limit {
        CheckOutcome::Fail
    } else if limit.saturating_sub(used) < threshold {
        CheckOutcome::Warn
    } else {
        CheckOutcome::Pass
    };
    let headroom = limit.saturating_sub(used);
    let message = match outcome {
        CheckOutcome::Pass => format!("{} bytes used, {} headroom", used, headroom),
        CheckOutcome::Warn => format!(
            "{} bytes used, only {} headroom (under {} threshold — fits today, may not survive growth)",
            used, headroom, threshold
        ),
        CheckOutcome::Fail => format!("{} bytes used, {} OVER ({})", used, used - limit, what),
    };
    CheckResult {
        outcome,
        fits: outcome != CheckOutcome::Fail,
        used,
        limit,
        headroom_threshold_bytes: threshold,
        message,
    }
}
