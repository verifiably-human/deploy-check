// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0

//! Ledger 9 backend (midnight-ledger `ledger-9.1.0.0-rc.4`, commit
//! cd652d7f97b34b805bb5f0310ce7434eb883af38).
//!
//! Builds the deploy-only transaction a wallet would submit, before fees,
//! and asks the ledger for its cost (`Transaction::cost`). The contract state
//! is the build's real initial state (constructor ledger data plus one
//! operation per circuit, see `scripts/initial-state.mjs`), with each
//! circuit's verifier key put in the slot its tag selects: ZKIR v2 keys
//! (`verifier-key[v6]`, compactc's default) in `v2`, ZKIR v3 keys
//! (`verifier-key[v7]`, `--feature-zkir-v3`) in `v3`. IR is not attached;
//! it is installed separately (`SingleUpdate::IrInsert`).
//!
//! Costing only `tree_copy` of a state with null ledger data, as earlier
//! versions did, undercounted by 1.6-2.1 KB on 17- and 18-circuit builds
//! and passed a deploy the chain rejected.

use super::{DeployOptions, ExactCost};
use crate::artifacts::Artifacts;
use anyhow::{anyhow, bail, Context, Result};

use base_crypto_l9::schnorr::SigningKey;
use base_crypto_l9::time::Timestamp;
use ledger_l9::structure::{
    ContractDeploy, Intent, ProofPreimageMarker, Signature, Transaction, INITIAL_PARAMETERS,
};
use onchain_state_l9::state::{
    ContractMaintenanceAuthority, ContractMaintenanceVerifyingKey, ContractState, EntryPointBuf,
};
use rand::rngs::OsRng;
use serialize_l9::tagged_deserialize;
use storage_l9::db::InMemoryDB;
use transient_crypto_l9::commitment::PedersenRandomness;

const V6_TAG: &[u8] = b"midnight:verifier-key[v6]:";
const V7_TAG: &[u8] = b"midnight:verifier-key[v7]:";

type Tx = Transaction<Signature, ProofPreimageMarker, PedersenRandomness, InMemoryDB>;

pub fn deploy_cost(
    artifacts: &Artifacts,
    initial_state: &[u8],
    options: &DeployOptions,
) -> Result<ExactCost> {
    let mut state: ContractState<InMemoryDB> = tagged_deserialize(&mut &initial_state[..])
        .context("deserializing the initial ContractState as ledger 9")?;

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
        let bytes = &circuit.verifier_key_bytes[..];
        if bytes.starts_with(V6_TAG) {
            op.v2 = Some(tagged_deserialize(&mut &bytes[..]).with_context(|| {
                format!("deserializing ZKIR v2 verifier key for `{}`", circuit.name)
            })?);
        } else if bytes.starts_with(V7_TAG) {
            op.v3 = Some(tagged_deserialize(&mut &bytes[..]).with_context(|| {
                format!("deserializing ZKIR v3 verifier key for `{}`", circuit.name)
            })?);
        } else {
            bail!(
                "verifier key for circuit `{}` has an unrecognized tag; this backend knows verifier-key[v6] (ZKIR v2) and verifier-key[v7] (ZKIR v3)",
                circuit.name
            );
        }
        state.operations = state.operations.insert(entry_point, op);
    }

    if options.authority_keys > 0 {
        let committee = (0..options.authority_keys)
            .map(|_| {
                ContractMaintenanceVerifyingKey::Schnorr(SigningKey::sample(OsRng).verifying_key())
            })
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
