// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0

//! SARIF 2.1.0 emission. See specs/SPEC-2-sarif.md.
//!
//! Maps each failed ceiling check in a deploy-check report to a SARIF
//! `result`. Reports that pass produce a SARIF run with an empty
//! results array (still uploadable; GitHub Code Scanning recognises
//! this as "no findings on this run").

use crate::report::{CheckResult, Report};
use serde::Serialize;
use std::path::Path;

const SARIF_SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";
const TOOL_INFO_URI: &str = "https://github.com/verifiably-human/deploy-check";

#[derive(Serialize)]
pub struct SarifLog<'a> {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<SarifRun<'a>>,
}

#[derive(Serialize)]
struct SarifRun<'a> {
    tool: SarifTool<'a>,
    results: Vec<SarifResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "originalUriBaseIds")]
    original_uri_base_ids: Option<UriBaseIds>,
}

#[derive(Serialize)]
struct SarifTool<'a> {
    driver: SarifDriver<'a>,
}

#[derive(Serialize)]
struct SarifDriver<'a> {
    name: &'static str,
    version: &'a str,
    #[serde(rename = "informationUri")]
    information_uri: &'static str,
    rules: Vec<SarifRule>,
}

#[derive(Serialize)]
struct SarifRule {
    id: &'static str,
    name: &'static str,
    #[serde(rename = "shortDescription")]
    short_description: TextBlock,
    #[serde(rename = "fullDescription")]
    full_description: TextBlock,
    #[serde(rename = "defaultConfiguration")]
    default_configuration: LevelBlock,
    #[serde(rename = "helpUri")]
    help_uri: &'static str,
}

#[derive(Serialize)]
struct TextBlock {
    text: &'static str,
}

#[derive(Serialize)]
struct LevelBlock {
    level: &'static str,
}

#[derive(Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: &'static str,
    level: &'static str,
    message: Message,
    locations: Vec<Location>,
    #[serde(rename = "partialFingerprints")]
    partial_fingerprints: PartialFingerprints,
    properties: Properties,
}

#[derive(Serialize)]
struct Message {
    text: String,
}

#[derive(Serialize)]
struct Location {
    #[serde(rename = "physicalLocation")]
    physical_location: PhysicalLocation,
}

#[derive(Serialize)]
struct PhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: ArtifactLocation,
}

#[derive(Serialize)]
struct ArtifactLocation {
    uri: String,
    #[serde(rename = "uriBaseId")]
    uri_base_id: &'static str,
}

#[derive(Serialize)]
struct PartialFingerprints {
    #[serde(rename = "contractName/v1")]
    contract_name: String,
    #[serde(rename = "ceiling/v1")]
    ceiling: &'static str,
}

#[derive(Serialize)]
struct Properties {
    used_bytes: u64,
    limit_bytes: u64,
    overage_bytes: u64,
    mode: &'static str,
}

#[derive(Serialize)]
struct UriBaseIds {
    #[serde(rename = "SRC")]
    src: UriEntry,
}

#[derive(Serialize)]
struct UriEntry {
    uri: String,
}

fn rules_catalog() -> Vec<SarifRule> {
    vec![
        SarifRule {
            id: "ceiling:block-bytes-written",
            name: "BlockBytesWrittenExceeded",
            short_description: TextBlock { text: "Deploy exceeds per-block bytes_written budget" },
            full_description: TextBlock {
                text: "The contract's serialized state plus tree pointer overhead exceeds the ledger's per-block bytes_written ceiling (50 KiB on mainnet at ledger-8.1.0). The deploy transaction will be rejected.",
            },
            default_configuration: LevelBlock { level: "error" },
            help_uri: "https://github.com/verifiably-human/deploy-check#ceilings",
        },
        SarifRule {
            id: "ceiling:transaction-byte-limit",
            name: "TransactionByteLimitExceeded",
            short_description: TextBlock { text: "Deploy transaction exceeds transaction byte limit" },
            full_description: TextBlock {
                text: "The serialized transaction carrying this deploy exceeds the per-transaction byte limit (1 MiB at ledger-8.1.0). Rare for single contracts; usually caused by many-circuit contracts or oversized verifier keys.",
            },
            default_configuration: LevelBlock { level: "error" },
            help_uri: "https://github.com/verifiably-human/deploy-check#ceilings",
        },
        SarifRule {
            id: "ceiling:per-entry-point-metadata",
            name: "PerEntryPointMetadataExceeded",
            short_description: TextBlock { text: "A single circuit's metadata exceeds the per-entry-point cap" },
            full_description: TextBlock {
                text: "One circuit's (verifier_key + ContractOperation framing) bundle exceeds the ledger's per-entry-point metadata cap (10 MiB at ledger-8.1.0). Almost never the binding constraint; usually a sign of an unusually large circuit.",
            },
            default_configuration: LevelBlock { level: "error" },
            help_uri: "https://github.com/verifiably-human/deploy-check#ceilings",
        },
    ]
}

/// Produce a SARIF result for a single failed ceiling check. Returns
/// None if the check passed.
fn result_for_ceiling(
    rule_id: &'static str,
    ceiling_key: &'static str,
    check: &CheckResult,
    contract_uri: &str,
    contract_name: &str,
    mode: &'static str,
) -> Option<SarifResult> {
    if check.fits {
        return None;
    }
    let overage = check.used.saturating_sub(check.limit);
    Some(SarifResult {
        rule_id,
        level: "error",
        message: Message {
            text: format!(
                "Deploy exceeds {} budget: {} / {} (overage: {} bytes)",
                rule_id, check.used, check.limit, overage
            ),
        },
        locations: vec![Location {
            physical_location: PhysicalLocation {
                artifact_location: ArtifactLocation {
                    uri: contract_uri.to_string(),
                    uri_base_id: "SRC",
                },
            },
        }],
        partial_fingerprints: PartialFingerprints {
            contract_name: contract_name.to_string(),
            ceiling: ceiling_key,
        },
        properties: Properties {
            used_bytes: check.used,
            limit_bytes: check.limit,
            overage_bytes: overage,
            mode,
        },
    })
}

/// Build a SARIF 2.1.0 log document for the given deploy-check report.
///
/// `build_dir` is used to derive the contract name (last path
/// component) used in fingerprints and as the artifact URI. The
/// originalUriBaseIds entry points consumers at the directory itself.
pub fn build_log<'a>(
    report: &Report,
    build_dir: &Path,
    tool_version: &'a str,
) -> SarifLog<'a> {
    let contract_name = build_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let contract_uri = format!("{}/contract-info.json", "compiler");

    let mode = match report.totals.exact_bytes_written {
        Some(_) => "exact",
        None => "estimator",
    };

    let mut results = Vec::new();
    if let Some(r) = result_for_ceiling(
        "ceiling:block-bytes-written",
        "block-bytes-written",
        &report.checks.block_bytes_written,
        &contract_uri,
        &contract_name,
        mode,
    ) {
        results.push(r);
    }
    if let Some(r) = result_for_ceiling(
        "ceiling:transaction-byte-limit",
        "transaction-byte-limit",
        &report.checks.transaction_byte_limit,
        &contract_uri,
        &contract_name,
        mode,
    ) {
        results.push(r);
    }
    if let Some(r) = result_for_ceiling(
        "ceiling:per-entry-point-metadata",
        "per-entry-point-metadata",
        &report.checks.per_entry_point_metadata,
        &contract_uri,
        &contract_name,
        mode,
    ) {
        results.push(r);
    }

    SarifLog {
        schema: SARIF_SCHEMA,
        version: "2.1.0",
        runs: vec![SarifRun {
            tool: SarifTool {
                driver: SarifDriver {
                    name: "deploy-check",
                    version: tool_version,
                    information_uri: TOOL_INFO_URI,
                    rules: rules_catalog(),
                },
            },
            results,
            original_uri_base_ids: Some(UriBaseIds {
                src: UriEntry {
                    uri: build_dir.to_string_lossy().into_owned(),
                },
            }),
        }],
    }
}
