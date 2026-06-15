// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Exact `bytes_written` and serialized-size computation via the
//! midnight-ledger crate.
//!
//! Mirrors the canonical deploy cost at
//! `ledger/src/structure.rs::ContractAction::Deploy` exactly:
//!
//!   model.map_insert(EXPECTED_CONTRACT_DEPTH, false)
//!     + model.tree_copy(Sp::new(deploy.initial_state.clone()))
//!
//! `map_insert` accounts for inserting the new contract entry into the
//! contracts registry map; `tree_copy` accounts for writing the
//! contract's own state. Earlier versions of this tool computed only
//! the `tree_copy` term, under-reporting by a few hundred bytes.
//!
//! Also returns the real serialized size of the wrapping
//! `ContractDeploy` so the transaction-byte-limit check uses a measured
//! number, not the estimator's constant.
//!
//! Only compiled when the `exact` Cargo feature is enabled.

use crate::artifacts::Artifacts;
use anyhow::{Context, Result};

use midnight_base_crypto::hash::PERSISTENT_HASH_BYTES;
use midnight_ledger::structure::ContractDeploy;
use midnight_onchain_state::state::{
    ContractOperation, ContractState, EntryPointBuf, StateValue,
};
use midnight_onchain_vm::cost_model::INITIAL_COST_MODEL;
use midnight_serialize::{tagged_deserialize, tagged_serialize};
use midnight_storage::{arena::Sp, db::InMemoryDB, storage::HashMap as StorageHashMap};
use midnight_transient_crypto::proofs::VerifierKey;
use rand::thread_rng;

/// Depth at which a contract's entry is stored in the contracts
/// registry map. Mirrors `EXPECTED_CONTRACT_DEPTH` (pub(crate)) at
/// `ledger/src/structure.rs:1816` of midnight-ledger@ledger-8.1.0.
/// If a future ledger release changes it, update here in lockstep.
const EXPECTED_CONTRACT_DEPTH: usize = 32;

/// Replicates the `bytes_written` contribution of
/// `CostModel::map_insert`, which is `pub(crate)` upstream so external
/// consumers can't call it directly. The formula:
///
///   layers = log_size.div_ceil(4)
///   bytes_written = (PERSISTENT_HASH_BYTES * 16) * layers + PERSISTENT_HASH_BYTES
///
/// `overwrite` does not affect `bytes_written` in cell_write (only
/// `bytes_deleted`), so the parameter is irrelevant for this use case.
///
/// SOURCE: ledger/src/structure.rs:1028 (cell_write) and :1049 (map_insert)
/// in midnight-ledger@ledger-8.1.0. Re-check if the dep is bumped.
fn map_insert_bytes_written(log_size: usize) -> u64 {
    let layers = log_size.div_ceil(4) as u64;
    let phb = PERSISTENT_HASH_BYTES as u64;
    (phb * 16) * layers + phb
}

/// Outcome of running the real ledger cost model over the contract's
/// initial state and the wrapping ContractDeploy.
#[derive(Debug, Clone, Copy)]
pub struct ExactCost {
    /// `map_insert + tree_copy` bytes_written total. This is the value
    /// the ledger charges against the per-block bytes_written budget.
    pub bytes_written: u64,
    /// `tree_copy` contribution alone. Useful for the report breakdown.
    pub tree_copy_bytes_written: u64,
    /// `map_insert` contribution alone.
    pub map_insert_bytes_written: u64,
    /// Length in bytes of the tagged-serialized `ContractDeploy`. The
    /// transaction byte limit applies to the wrapping transaction, but
    /// the deploy itself is the dominant contributor; this value is a
    /// tight lower bound on the final tx size.
    pub deploy_serialized_bytes: u64,
}

/// Build a `ContractState` from the artifacts and compute its exact
/// deploy cost.
pub fn compute(artifacts: &Artifacts) -> Result<ExactCost> {
    let mut ops: StorageHashMap<EntryPointBuf, ContractOperation, InMemoryDB> =
        StorageHashMap::new();

    for circuit in &artifacts.circuits {
        let vk: VerifierKey = tagged_deserialize(&mut &circuit.verifier_key_bytes[..])
            .with_context(|| {
                format!("deserializing verifier key for circuit `{}`", circuit.name)
            })?;
        let op = ContractOperation::new(Some(vk));
        let entry_point: EntryPointBuf = circuit.name.as_bytes().into();
        ops = ops.insert(entry_point, op);
    }

    let state: ContractState<InMemoryDB> = ContractState::new(
        StateValue::Null,
        ops,
        Default::default(),
    );

    // Cost model: same call shape as the ledger's deploy branch
    // (ContractAction::Deploy → map_insert + tree_copy). map_insert is
    // pub(crate) upstream so we replicate the arithmetic.
    let tree_cost = INITIAL_COST_MODEL.tree_copy(Sp::new(state.clone()));
    let map_insert_bw = map_insert_bytes_written(EXPECTED_CONTRACT_DEPTH);

    // Real ContractDeploy → tagged_serialize for the transaction-byte
    // measurement. The nonce field is 32 bytes regardless of value, so
    // any RNG produces a deterministic length even with a non-
    // deterministic seed.
    let mut rng = thread_rng();
    let deploy: ContractDeploy<InMemoryDB> = ContractDeploy::new(&mut rng, state);
    let mut buf = Vec::new();
    tagged_serialize(&deploy, &mut buf)
        .context("serializing ContractDeploy for size measurement")?;

    Ok(ExactCost {
        bytes_written: tree_cost.bytes_written + map_insert_bw,
        tree_copy_bytes_written: tree_cost.bytes_written,
        map_insert_bytes_written: map_insert_bw,
        deploy_serialized_bytes: buf.len() as u64,
    })
}
