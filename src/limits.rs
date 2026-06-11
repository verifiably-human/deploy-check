// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0

//! Deploy-time ceilings. Mirrors `midnight-ledger`'s `TransactionLimits`
//! and the `bytes_written` dimension of `block_limits`. Limits are taken
//! verbatim from `INITIAL_LIMITS`; governance can raise them via
//! `SystemTransaction::OverwriteParameters` so non-mainnet networks may
//! diverge.

use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Limits {
    /// Maximum serialized transaction size in bytes. INITIAL_LIMITS: 1 MiB.
    pub transaction_byte_limit: u64,
    /// Per `(EntryPoint, ContractOperation)` metadata cap. INITIAL_LIMITS: 10 MiB.
    pub max_contract_metadata_size: u64,
    /// Block-cost `bytes_written` budget. INITIAL_LIMITS: 50 KiB.
    pub block_bytes_written: u64,
}

impl Limits {
    pub const INITIAL: Self = Self {
        transaction_byte_limit: 1 << 20,
        max_contract_metadata_size: 10 * 1024 * 1024,
        block_bytes_written: 50_000,
    };
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum NetworkPreset {
    /// Mainnet uses INITIAL_LIMITS verbatim.
    Mainnet,
    /// Preview tracks mainnet until governance diverges.
    Preview,
    /// QAnet matches mainnet defaults.
    Qanet,
    /// Local devnet matches mainnet defaults unless overridden.
    Local,
}

impl NetworkPreset {
    pub fn limits(&self) -> Limits {
        // All networks currently track INITIAL_LIMITS. If governance
        // raises limits on a network, update this match.
        match self {
            Self::Mainnet | Self::Preview | Self::Qanet | Self::Local => Limits::INITIAL,
        }
    }
}

pub fn load_from_file(path: &Path) -> Result<Limits> {
    let s = std::fs::read_to_string(path)?;
    let l: Limits = serde_json::from_str(&s)?;
    Ok(l)
}
