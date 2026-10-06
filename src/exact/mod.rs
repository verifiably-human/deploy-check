// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0

//! Exact `bytes_written` via a real midnight-ledger cost model.
//!
//! One backend per ledger major version, each behind its own Cargo feature,
//! because the ledger crates change shape between majors (ledger 9 splits
//! `ContractOperation` into a v2 slot for ZKIR v2 keys, a v3 slot for ZKIR v3
//! keys, and an optional IR). Pick the backend at run time with `--ledger`.
//!
//! The deploy branch of the ledger's fee computation charges
//! `map_index(EXPECTED_CONTRACT_DEPTH) + map_insert(EXPECTED_CONTRACT_DEPTH,
//! false) + tree_copy(initial_state)` (ledger/src/structure.rs, the
//! `ContractAction::Deploy` arm; identical in ledger-8.1.0 and
//! ledger-9.1.0.0-rc.4). `map_index` is read-only. `map_insert(32, false)`
//! writes eight 512-byte layers it also overwrites, plus one 32-byte leaf;
//! after the `RunningCost` to `SyntheticCost` conversion nets deleted bytes
//! out, that is +32 bytes of `bytes_written`. The backends add it.

use crate::artifacts::Artifacts;
use anyhow::{bail, Result};

#[cfg(feature = "exact-ledger8")]
mod ledger8;
#[cfg(feature = "exact-ledger9")]
mod ledger9;

/// Net `bytes_written` of `map_insert(EXPECTED_CONTRACT_DEPTH, false)`:
/// the 32-byte leaf (`PERSISTENT_HASH_BYTES`). The 4,096 bytes of
/// overwritten layers count as churn, not writes.
pub const DEPLOY_MAP_INSERT_NET_BYTES: u64 = 32;

/// Ledger major versions with an exact backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LedgerVersion {
    #[value(name = "8")]
    V8,
    #[value(name = "9")]
    V9,
}

/// Backends compiled into this binary.
pub fn available() -> Vec<LedgerVersion> {
    #[allow(unused_mut)]
    let mut v = Vec::new();
    #[cfg(feature = "exact-ledger8")]
    v.push(LedgerVersion::V8);
    #[cfg(feature = "exact-ledger9")]
    v.push(LedgerVersion::V9);
    v
}

/// Exact deploy `bytes_written`: `tree_copy(initial_state)` plus the
/// operations-map insertion.
#[derive(Debug, Clone, Copy)]
pub struct ExactCost {
    pub bytes_written: u64,
}

pub fn compute(
    artifacts: &Artifacts,
    requested: Option<LedgerVersion>,
) -> Result<(LedgerVersion, ExactCost)> {
    let avail = available();
    let version = match (requested, avail.as_slice()) {
        (_, []) => bail!(
            "--exact needs a ledger backend. Rebuild with `--features exact-ledger8` (alias `exact`) or `--features exact-ledger9`, e.g.\n  cargo install --git https://github.com/verifiably-human/deploy-check --features exact-ledger8"
        ),
        (Some(v), _) if avail.contains(&v) => v,
        (Some(v), _) => bail!("--ledger {v:?} was not compiled in; this binary has {avail:?}"),
        (None, [only]) => *only,
        (None, _) => bail!(
            "this binary has several ledger backends ({avail:?}); choose one with --ledger"
        ),
    };
    let tree_copy = tree_copy_for(version, artifacts)?;
    Ok((
        version,
        ExactCost {
            bytes_written: tree_copy + DEPLOY_MAP_INSERT_NET_BYTES,
        },
    ))
}

#[cfg(any(feature = "exact-ledger8", feature = "exact-ledger9"))]
fn tree_copy_for(version: LedgerVersion, artifacts: &Artifacts) -> Result<u64> {
    match version {
        #[cfg(feature = "exact-ledger8")]
        LedgerVersion::V8 => ledger8::tree_copy_bytes_written(artifacts),
        #[cfg(feature = "exact-ledger9")]
        LedgerVersion::V9 => ledger9::tree_copy_bytes_written(artifacts),
        #[allow(unreachable_patterns)]
        other => bail!("ledger {other:?} backend not compiled in"),
    }
}

#[cfg(not(any(feature = "exact-ledger8", feature = "exact-ledger9")))]
fn tree_copy_for(_version: LedgerVersion, _artifacts: &Artifacts) -> Result<u64> {
    unreachable!("compute() bails before this when no backend is compiled in")
}
