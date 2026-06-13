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

//! Exact `bytes_written` computation via the midnight-ledger crate.
//!
//! Replaces the heuristic estimator with a real `tree_copy` invocation
//! over a `ContractState` constructed from the build directory.
//!
//! Only compiled when the `exact` Cargo feature is enabled — the ledger
//! workspace pulls in eight crates and a multi-minute cold build, so
//! default users get the lightweight estimator.

use crate::artifacts::Artifacts;
use anyhow::{Context, Result};

use midnight_serialize::tagged_deserialize;
use midnight_onchain_state::state::{
    ContractOperation, ContractState, EntryPointBuf, StateValue,
};
use midnight_onchain_vm::cost_model::INITIAL_COST_MODEL;
use midnight_storage::{db::InMemoryDB, storage::HashMap as StorageHashMap, arena::Sp};
use midnight_transient_crypto::proofs::VerifierKey;

/// Outcome of a real `cost_model.tree_copy` over the contract's initial
/// state. All fields come directly from `RunningCost.bytes_written`.
#[derive(Debug, Clone, Copy)]
pub struct ExactCost {
    /// `tree_copy.bytes_written` — the on-chain bytes written when the
    /// deploy commits. This is what the per-block `bytes_written` budget
    /// constrains.
    pub bytes_written: u64,
}

/// Build a `ContractState` from the artifacts and compute its exact
/// `tree_copy` cost.
///
/// Mirrors the canonical site in `ledger/src/structure.rs` (the
/// `ContractAction::Deploy` cost branch).
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

    let cost = INITIAL_COST_MODEL.tree_copy(Sp::new(state.clone()));

    Ok(ExactCost {
        bytes_written: cost.bytes_written,
    })
}
