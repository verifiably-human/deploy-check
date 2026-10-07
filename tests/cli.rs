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
#[cfg(not(any(feature = "exact-ledger8", feature = "exact-ledger9")))]
fn exact_flag_without_feature_errors_clearly() {
    let fixture = build_fixture("Small", &[("foo", 2875)]);
    bin()
        .arg(fixture.path())
        .arg("--exact")
        .assert()
        .failure()
        .stderr(contains("--exact needs a ledger backend"));
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

// Regression: the Dominion deployments (gist ADGLx/c9307e5e, 2026-09-30).
// Charged bytes_written as reported by the chain: the 9-circuit Deck was
// confirmed; the 12- and 14-circuit merged Tables were rejected with
// "Transaction would exhaust the block limits". All three fit the 50,000
// byte block budget, which is why deploy-check 0.1 passed all three.
fn dominion(bytes_written: u64) -> assert_cmd::assert::Assert {
    let fixture = build_fixture("Dominion", &[("c0", 2875)]);
    bin()
        .arg(fixture.path())
        .arg("--bytes-written")
        .arg(bytes_written.to_string())
        .assert()
}

#[test]
fn dominion_deck_27978_fits() {
    dominion(27_978)
        .success()
        .stdout(contains("[PASS] per-transaction weight"))
        .stdout(contains("Result: FITS"));
}

#[test]
fn dominion_trimmed_table_35143_fails_per_tx_but_passes_block_budget() {
    dominion(35_143)
        .failure()
        .stdout(contains("[FAIL] per-transaction weight"))
        .stdout(contains("[PASS] block bytes_written"))
        .stdout(contains("Transaction would exhaust the block limits"));
}

#[test]
fn dominion_full_table_38168_fails_per_tx_but_passes_block_budget() {
    dominion(38_168)
        .failure()
        .stdout(contains("[FAIL] per-transaction weight"))
        .stdout(contains("[PASS] block bytes_written"));
}

#[test]
fn per_tx_allowance_is_31997_bytes_written_in_json() {
    let fixture = build_fixture("Small", &[("foo", 2875)]);
    let out = bin()
        .arg(fixture.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let w = &v["checks"]["per_transaction_weight"];
    assert_eq!(w["binding_dimension"], "bytes_written");
    assert_eq!(w["limit"], 31_997);
    assert_eq!(w["max_extrinsic_ps"], 1_299_891_843_000u64);
}

#[test]
fn boundary_31997_fits_and_31998_does_not() {
    dominion(31_997).success();
    dominion(31_998).failure();
}

#[test]
fn weight_overrides_move_the_ceiling() {
    // A 90% normal dispatch ratio raises max_extrinsic to ~1.6 s, which
    // admits the 35,143-byte deploy.
    let fixture = build_fixture("Dominion", &[("c0", 2875)]);
    bin()
        .arg(fixture.path())
        .args([
            "--bytes-written",
            "35143",
            "--normal-dispatch-percent",
            "90",
        ])
        .assert()
        .success()
        .stdout(contains("[PASS] per-transaction weight"));
}

#[test]
fn accepts_node_ledger_parameters_config_file() {
    // Shape of midnight-node res/<network>/ledger-parameters-config.json
    // (cost_model trimmed; it is ignored).
    let fixture = build_fixture("Small", &[("foo", 2875)]);
    let file = fixture.path().join("ledger-parameters-config.json");
    fs::write(
        &file,
        r#"{
  "cost_model": {},
  "limits": {
    "transaction_byte_limit": 1048576,
    "time_to_dismiss_per_byte": 2000000,
    "min_time_to_dismiss": 15000000000,
    "block_limits": {
      "readTime": 2000000000000,
      "computeTime": 2000000000000,
      "blockUsage": 1000000,
      "bytesWritten": 60000,
      "bytesChurned": 50000000
    },
    "block_withdrawal_minimum_multiple": 9223372036854775808,
    "max_contract_metadata_size": 10485760
  }
}"#,
    )
    .unwrap();
    // 60,000 bytes_written per block gives a per-tx allowance of 38,396,
    // so 38,168 now fits.
    bin()
        .arg(fixture.path())
        .arg("--limits-file")
        .arg(&file)
        .args(["--bytes-written", "38168"])
        .assert()
        .success()
        .stdout(contains("[PASS] per-entry-point cap"));
}

#[test]
fn accepts_legacy_flat_limits_file() {
    let fixture = build_fixture("Small", &[("foo", 2875)]);
    let file = fixture.path().join("legacy.json");
    fs::write(
        &file,
        r#"{"transaction_byte_limit":1048576,"max_contract_metadata_size":10485760,"block_bytes_written":50000}"#,
    )
    .unwrap();
    bin()
        .arg(fixture.path())
        .arg("--limits-file")
        .arg(&file)
        .assert()
        .success()
        .stdout(contains("legacy flat format"));
}

#[test]
fn unknown_network_lists_presets() {
    let fixture = build_fixture("Small", &[("foo", 2875)]);
    bin()
        .arg(fixture.path())
        .args(["--network", "nosuchnet"])
        .assert()
        .failure()
        .stderr(contains("available presets"))
        .stderr(contains("preprod"));
}

#[test]
fn list_networks_prints_presets() {
    bin()
        .arg("--list-networks")
        .assert()
        .success()
        .stdout(contains("mainnet"))
        .stdout(contains("stagenet"));
}

#[test]
fn mainnet_preset_skips_metadata_cap_it_does_not_set() {
    let fixture = build_fixture("Small", &[("foo", 2875)]);
    bin()
        .arg(fixture.path())
        .assert()
        .success()
        .stdout(contains("[SKIP] per-entry-point cap"));
}

// Exact mode against the ledger's own `Transaction::cost`. Each fixture is a
// one-circuit build (contract-info.json, the verifier key) plus its initial
// ContractState from scripts/initial-state.mjs. The expected figures are what
// the ledger's JS bindings report for the same deploy-only transaction:
// ledger-v9 1.0.0-rc.5 for the ledger 9 build (compactc 0.34.0), and
// ledger-v8 8.1.0 for the ledger 8 build (compactc 0.31.1). On a ledger 9
// devnet the chain charged exactly these figures plus 128 bytes of fee spend.
#[cfg(any(feature = "exact-ledger8", feature = "exact-ledger9"))]
fn exact_bytes(fixture: &str, ledger: &str, extra: &[&str]) -> u64 {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let out = bin()
        .arg(&dir)
        .args(["--exact", "--ledger", ledger, "--json", "--initial-state"])
        .arg(dir.join("initial-state.bin"))
        .args(extra)
        .output()
        .expect("run deploy-check");
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json report");
    parsed["totals"]["exact_bytes_written"]
        .as_u64()
        .expect("exact figure")
}

#[test]
#[cfg(feature = "exact-ledger9")]
fn exact_ledger9_matches_ledger_transaction_cost() {
    let no_extras = ["--authority-keys", "0", "--fee-allowance", "0"];
    assert_eq!(exact_bytes("l9-n1", "9", &no_extras), 2014);
}

#[test]
#[cfg(feature = "exact-ledger9")]
fn exact_ledger9_adds_fee_allowance_and_authority() {
    let base = exact_bytes(
        "l9-n1",
        "9",
        &["--authority-keys", "0", "--fee-allowance", "0"],
    );
    assert_eq!(
        exact_bytes("l9-n1", "9", &["--authority-keys", "0"]),
        base + 128
    );
    assert!(exact_bytes("l9-n1", "9", &["--fee-allowance", "0"]) > base);
}

#[test]
#[cfg(feature = "exact-ledger8")]
fn exact_ledger8_matches_ledger_transaction_cost() {
    let no_extras = ["--authority-keys", "0", "--fee-allowance", "0"];
    assert_eq!(exact_bytes("l8-n1", "8", &no_extras), 2012);
}
