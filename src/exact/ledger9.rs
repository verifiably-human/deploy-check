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
//! Ledger 9's `ContractOperation` has a `v2` slot for keys from ZKIR v2
//! circuits (compactc's default; file tag `verifier-key[v6]`, parsed with
//! midnight-transient-crypto 2.x), a `v3` slot for ZKIR v3 keys
//! (`--feature-zkir-v3`; tag `verifier-key[v7]`, transient-crypto 3.x), and
//! an optional IR. Each key goes into the slot its tag selects. IR is not
//! attached: it is installed separately (`SingleUpdate::IrInsert`), not as
//! part of a deploy.

use crate::artifacts::Artifacts;
use anyhow::{bail, Context, Result};

use onchain_state_l9::state::{ContractOperation, ContractState, EntryPointBuf, StateValue};
use onchain_vm_l9::cost_model::INITIAL_COST_MODEL;
use serialize_l9::tagged_deserialize;
use storage_l9::{arena::Sp, db::InMemoryDB, storage::HashMap as StorageHashMap};

const V6_TAG: &[u8] = b"midnight:verifier-key[v6]:";
const V7_TAG: &[u8] = b"midnight:verifier-key[v7]:";

pub fn tree_copy_bytes_written(artifacts: &Artifacts) -> Result<u64> {
    let mut ops: StorageHashMap<EntryPointBuf, ContractOperation, InMemoryDB> =
        StorageHashMap::new();

    for circuit in &artifacts.circuits {
        let bytes = &circuit.verifier_key_bytes[..];
        let mut op = ContractOperation::new(None, None);
        if bytes.starts_with(V6_TAG) {
            let vk: transient_crypto_l9_old::proofs::VerifierKey =
                tagged_deserialize(&mut &bytes[..]).with_context(|| {
                    format!(
                        "deserializing ZKIR v2 verifier key for circuit `{}`",
                        circuit.name
                    )
                })?;
            op.v2 = Some(vk);
        } else if bytes.starts_with(V7_TAG) {
            let vk: transient_crypto_l9::proofs::VerifierKey = tagged_deserialize(&mut &bytes[..])
                .with_context(|| {
                    format!(
                        "deserializing ZKIR v3 verifier key for circuit `{}`",
                        circuit.name
                    )
                })?;
            op.v3 = Some(vk);
        } else {
            bail!(
                "verifier key for circuit `{}` has an unrecognized tag; this backend knows verifier-key[v6] (ZKIR v2) and verifier-key[v7] (ZKIR v3)",
                circuit.name
            );
        }
        let entry_point: EntryPointBuf = circuit.name.as_bytes().into();
        ops = ops.insert(entry_point, op);
    }

    let state: ContractState<InMemoryDB> =
        ContractState::new(StateValue::Null, ops, Default::default());

    Ok(INITIAL_COST_MODEL.tree_copy(Sp::new(state)).bytes_written)
}
