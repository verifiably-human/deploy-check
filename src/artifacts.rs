// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0

//! Reads a compactc output directory and assembles a `ContractDeploy`
//! whose serialized size and `tree_copy` `bytes_written` cost can be
//! computed exactly.
//!
//! Expected directory layout:
//!
//! ```text
//! <build_dir>/
//!   compiler/
//!     contract-info.json
//!     contract-manifest.json
//!     security-analysis.json   (optional)
//!   zkir/
//!     <circuit>.zkir            for each circuit
//!   keys/
//!     <circuit>.verifier        for each circuit; produced by `zkir compile-many`
//!     <circuit>.prover          unused here
//! ```

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
#[allow(dead_code)] // build_dir/contract_info kept for forthcoming exact-mode work
pub struct Artifacts {
    pub build_dir: PathBuf,
    pub contract_info: ContractInfo,
    /// One entry per circuit declared as a top-level `export circuit ...`.
    pub circuits: Vec<CircuitArtifact>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // witnesses/contracts kept for forthcoming checks
pub struct ContractInfo {
    pub circuits: Vec<CircuitDecl>,
    pub witnesses: Vec<serde_json::Value>,
    pub contracts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // pure flag kept for forthcoming per-circuit reporting
pub struct CircuitDecl {
    pub name: String,
    #[serde(default)]
    pub pure: bool,
}

#[derive(Debug)]
pub struct CircuitArtifact {
    pub name: String,
    /// Bytes of the verifier-key file (`<name>.verifier`). This is the
    /// `verifier_key` field that ends up inside `ContractOperation`.
    pub verifier_key_bytes: Vec<u8>,
}

pub fn load(build_dir: &Path) -> Result<Artifacts> {
    let info_path = build_dir.join("compiler").join("contract-info.json");
    let info_text = fs::read_to_string(&info_path)
        .with_context(|| format!("reading {}", info_path.display()))?;
    let contract_info: ContractInfo = serde_json::from_str(&info_text)
        .with_context(|| format!("parsing {}", info_path.display()))?;

    let keys_dir = build_dir.join("keys");
    if !keys_dir.is_dir() {
        return Err(anyhow!(
            "no keys/ directory under {}; did you compile with the full zkir step?",
            build_dir.display()
        ));
    }

    let mut circuits = Vec::with_capacity(contract_info.circuits.len());
    for decl in &contract_info.circuits {
        let vk_path = keys_dir.join(format!("{}.verifier", decl.name));
        let vk_bytes = fs::read(&vk_path)
            .with_context(|| format!("reading verifier key {}", vk_path.display()))?;
        circuits.push(CircuitArtifact {
            name: decl.name.clone(),
            verifier_key_bytes: vk_bytes,
        });
    }

    Ok(Artifacts {
        build_dir: build_dir.to_path_buf(),
        contract_info,
        circuits,
    })
}
