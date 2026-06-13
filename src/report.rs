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
    /// Estimated serialized transaction size.
    pub estimated_tx_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Checks {
    pub block_bytes_written: CheckResult,
    pub transaction_byte_limit: CheckResult,
    pub per_entry_point_metadata: CheckResult,
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
        self.checks.block_bytes_written.fits
            && self.checks.transaction_byte_limit.fits
            && self.checks.per_entry_point_metadata.fits
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
        match self.totals.exact_bytes_written {
            Some(exact) => writeln!(
                out,
                "Totals: VK sum = {} bytes; exact bytes_written = {} bytes (estimator said {})",
                self.totals.verifier_key_bytes_total, exact, self.totals.estimated_bytes_written
            )?,
            None => writeln!(
                out,
                "Totals: VK sum = {} bytes; estimated bytes_written = {} bytes (run with --exact for the real value)",
                self.totals.verifier_key_bytes_total, self.totals.estimated_bytes_written
            )?,
        }
        writeln!(out)?;
        let pf = |c: &CheckResult| if c.fits { "PASS" } else { "FAIL" };
        writeln!(
            out,
            "[{}] block bytes_written:   {} / {}  ({})",
            pf(&self.checks.block_bytes_written),
            self.checks.block_bytes_written.used,
            self.checks.block_bytes_written.limit,
            self.checks.block_bytes_written.message
        )?;
        writeln!(
            out,
            "[{}] transaction size:      {} / {}  ({})",
            pf(&self.checks.transaction_byte_limit),
            self.checks.transaction_byte_limit.used,
            self.checks.transaction_byte_limit.limit,
            self.checks.transaction_byte_limit.message
        )?;
        writeln!(
            out,
            "[{}] per-entry-point cap:   {} / {}  ({})",
            pf(&self.checks.per_entry_point_metadata),
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

    // The block-bytes-written check uses the exact value when available;
    // otherwise it falls back to the estimator. Headroom labels reflect
    // which source produced the number.
    let (bytes_written_used, bytes_written_source) = match exact_bytes_written {
        Some(b) => (b, "exact bytes_written from cost_model.tree_copy"),
        None => (
            estimated_bytes_written,
            "estimated bytes_written from VK sum + tree overhead",
        ),
    };

    let block_bytes_written = check(
        bytes_written_source,
        bytes_written_used,
        limits.block_bytes_written,
    );
    let transaction_byte_limit = check(
        "estimated serialized transaction size",
        estimated_tx_bytes,
        limits.transaction_byte_limit,
    );
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
            exact_bytes_written,
            estimated_tx_bytes,
        },
        checks: Checks {
            block_bytes_written,
            transaction_byte_limit,
            per_entry_point_metadata,
        },
    }
}

/// MVP estimator. Mirrors `tree_copy` accounting from `cost_model.rs`:
/// bytes_written = sum(node.data.len + child_count * PERSISTENT_HASH_BYTES).
/// We model: per circuit, one operations-map entry with a child pointer
/// plus the VK bytes. Plus the ContractState wrapper.
fn estimate_bytes_written(circuit_count: u64, vk_sum: u64) -> u64 {
    let map_node_overhead = circuit_count * PERSISTENT_HASH_BYTES * 2;
    let per_entry_wrapper = circuit_count * ENTRY_POINT_OVERHEAD;
    vk_sum + map_node_overhead + per_entry_wrapper + CONTRACT_STATE_OVERHEAD
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
