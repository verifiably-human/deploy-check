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
//! Builds the deploy-only transaction a wallet would submit, before fees,
//! from the build's real initial state (constructor ledger data plus one
//! operation per circuit, see `scripts/initial-state.mjs`) with each
//! circuit's verifier key added, and asks the ledger for its cost
//! (`Transaction::cost`). Ledger 8 has one verifier-key slot per operation.

use super::{DeployOptions, ExactCost};
use crate::artifacts::Artifacts;
use anyhow::{anyhow, Context, Result};

use midnight_base_crypto::schnorr::Signature;
use midnight_base_crypto::schnorr::SigningKey;
use midnight_base_crypto::time::Timestamp;
use midnight_ledger::structure::{
    ContractDeploy, Intent, ProofPreimageMarker, Transaction, INITIAL_PARAMETERS,
};
use midnight_onchain_state::state::{ContractMaintenanceAuthority, ContractState, EntryPointBuf};
use midnight_serialize::tagged_deserialize;
use midnight_storage::db::InMemoryDB;
use midnight_transient_crypto::commitment::PedersenRandomness;
use midnight_transient_crypto::proofs::VerifierKey;
use rand::rngs::OsRng;

type Tx = Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>;

pub fn deploy_cost(
    artifacts: &Artifacts,
    initial_state: &[u8],
    options: &DeployOptions,
) -> Result<ExactCost> {
    let mut state: ContractState<InMemoryDB> = tagged_deserialize(&mut &initial_state[..])
        .context("deserializing the initial ContractState as ledger 8")?;

    for circuit in &artifacts.circuits {
        let entry_point: EntryPointBuf = circuit.name.as_bytes().into();
        let mut op = state
            .operations
            .get(&entry_point)
            .map(|op| (*op).clone())
            .ok_or_else(|| {
                anyhow!(
                    "the initial state has no operation for circuit `{}`; was it generated from this build?",
                    circuit.name
                )
            })?;
        let vk: VerifierKey = tagged_deserialize(&mut &circuit.verifier_key_bytes[..])
            .with_context(|| {
                format!(
                    "deserializing verifier key for circuit `{}` as a ledger 8 (ZKIR v2) key",
                    circuit.name
                )
            })?;
        op.v2 = Some(vk);
        state.operations = state.operations.insert(entry_point, op);
    }

    if options.authority_keys > 0 {
        let committee = (0..options.authority_keys)
            .map(|_| SigningKey::sample(OsRng).verifying_key())
            .collect::<Vec<_>>();
        state.maintenance_authority = ContractMaintenanceAuthority {
            committee,
            threshold: 1,
            counter: 0,
        };
    }

    let deploy = ContractDeploy::new(&mut OsRng, state);
    let ttl = Timestamp::from_secs(4_000_000_000);
    let intent = Intent::new(
        &mut OsRng,
        None,
        None,
        vec![],
        vec![],
        vec![deploy],
        None,
        ttl,
    );
    let tx: Tx = Transaction::from_intents("undeployed", [(1u16, intent)].into_iter().collect());

    let cost = tx
        .cost(&INITIAL_PARAMETERS, false)
        .map_err(|e| anyhow!("ledger cost computation failed: {e:?}"))?;
    Ok(ExactCost {
        bytes_written: cost.bytes_written,
    })
}
