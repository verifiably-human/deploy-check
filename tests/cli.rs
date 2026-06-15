// This file is part of deploy-check.
// Copyright (C) 2026 contributors to deploy-check
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//	http://www.apache.org/licenses/LICENSE-2.0

//! Integration tests for the deploy-check CLI binary.

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use tempfile::TempDir;

fn bin() -> Command {
    Command::cargo_bin("deploy-check").expect("binary should build")
}

fn build_fixture(name: &str, vk_sizes: &[(&str, usize)]) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    let compiler = dir.path().join("compiler");
    let keys = dir.path().join("keys");
    fs::create_dir_all(&compiler).unwrap();
    fs::create_dir_all(&keys).unwrap();

    let circuits: Vec<_> = vk_sizes
        .iter()
        .map(|(n, _)| serde_json::json!({ "name": n, "pure": false }))
        .collect();
    let info = serde_json::json!({
        "circuits": circuits,
        "witnesses": [],
        "contracts": [name],
    });
    fs::write(
        compiler.join("contract-info.json"),
        serde_json::to_string_pretty(&info).unwrap(),
    )
    .unwrap();

    for (circuit, size) in vk_sizes {
        // Filler bytes; the tool only needs the size, not the content.
        fs::write(keys.join(format!("{circuit}.verifier")), vec![0u8; *size]).unwrap();
    }

    dir
}

#[test]
fn reports_fits_for_small_contract() {
    let fixture = build_fixture("Small", &[("foo", 2875), ("bar", 2875)]);
    bin()
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(contains("Result: FITS"));
}

#[test]
fn reports_does_not_fit_when_over_block_budget() {
    // Force well over 50 KiB by piling on circuits.
    let circuits: Vec<(String, usize)> = (0..20).map(|i| (format!("c{i}"), 3000)).collect();
    let refs: Vec<(&str, usize)> = circuits.iter().map(|(n, s)| (n.as_str(), *s)).collect();
    let fixture = build_fixture("Big", &refs);
    bin()
        .arg(fixture.path())
        .assert()
        .failure()
        .stdout(contains("DOES NOT FIT"));
}

#[test]
fn json_mode_emits_machine_readable() {
    let fixture = build_fixture("Small", &[("foo", 2875)]);
    let out = bin()
        .arg(fixture.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    assert_eq!(parsed["totals"]["circuit_count"], 1);
    assert!(parsed["checks"]["block_bytes_written"]["fits"]
        .as_bool()
        .unwrap());
}

#[test]
#[cfg(not(feature = "exact"))]
fn exact_flag_without_feature_errors_clearly() {
    let fixture = build_fixture("Small", &[("foo", 2875)]);
    bin()
        .arg(fixture.path())
        .arg("--exact")
        .assert()
        .failure()
        .stderr(contains("--exact requires the `exact` cargo feature"));
}

#[test]
fn json_includes_outcome_field() {
    let fixture = build_fixture("Small", &[("foo", 2875)]);
    let out = bin()
        .arg(fixture.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    assert_eq!(parsed["checks"]["block_bytes_written"]["outcome"], "pass");
    // Deprecated `fits` boolean still emitted for back-compat.
    assert_eq!(parsed["checks"]["block_bytes_written"]["fits"], true);
    // Headroom threshold is reported.
    assert!(parsed["checks"]["block_bytes_written"]["headroom_threshold_bytes"]
        .as_u64()
        .unwrap() > 0);
}

#[test]
fn warns_when_headroom_under_threshold() {
    // 15 circuits at 3000 bytes each → just under the 50 KiB block limit.
    let circuits: Vec<(String, usize)> = (0..15).map(|i| (format!("c{i}"), 3000)).collect();
    let refs: Vec<(&str, usize)> = circuits.iter().map(|(n, s)| (n.as_str(), *s)).collect();
    let fixture = build_fixture("Tight", &refs);
    let out = bin()
        .arg(fixture.path())
        .arg("--json")
        .assert()
        .success() // still fits — exit 0
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    assert_eq!(parsed["checks"]["block_bytes_written"]["outcome"], "warn");
}

#[test]
fn missing_keys_dir_errors_clearly() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("compiler")).unwrap();
    fs::write(
        dir.path().join("compiler/contract-info.json"),
        r#"{"circuits":[],"witnesses":[],"contracts":[]}"#,
    )
    .unwrap();
    bin()
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(contains("no keys/ directory"));
}
