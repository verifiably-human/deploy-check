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

//! Ledger 8 backend (midnight-ledger `ledger-8.1.0`).
//!
//! Builds the `ContractState` a deploy would commit from the build
//! directory's verifier keys and charges it with `tree_copy`, mirroring the
//! `ContractAction::Deploy` cost branch in `ledger/src/structure.rs`.

use crate::artifacts::Artifacts;
use anyhow::{Context, Result};

use midnight_onchain_state::state::{ContractOperation, ContractState, EntryPointBuf, StateValue};
use midnight_onchain_vm::cost_model::INITIAL_COST_MODEL;
use midnight_serialize::tagged_deserialize;
use midnight_storage::{arena::Sp, db::InMemoryDB, storage::HashMap as StorageHashMap};
use midnight_transient_crypto::proofs::VerifierKey;

pub fn tree_copy_bytes_written(artifacts: &Artifacts) -> Result<u64> {
    let mut ops: StorageHashMap<EntryPointBuf, ContractOperation, InMemoryDB> =
        StorageHashMap::new();

    for circuit in &artifacts.circuits {
        let vk: VerifierKey = tagged_deserialize(&mut &circuit.verifier_key_bytes[..])
            .with_context(|| {
                format!(
                    "deserializing verifier key for circuit `{}` as a ledger 8 (ZKIR v2) key",
                    circuit.name
                )
            })?;
        let op = ContractOperation::new(Some(vk));
        let entry_point: EntryPointBuf = circuit.name.as_bytes().into();
        ops = ops.insert(entry_point, op);
    }

    let state: ContractState<InMemoryDB> =
        ContractState::new(StateValue::Null, ops, Default::default());

    Ok(INITIAL_COST_MODEL.tree_copy(Sp::new(state)).bytes_written)
}
