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

mod artifacts;
mod exact;
mod limits;
mod report;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "deploy-check", version, about, long_about = None)]
struct Cli {
    /// Path to the compactc output directory (containing compiler/, zkir/, keys/).
    #[arg(required_unless_present = "list_networks")]
    build_dir: Option<PathBuf>,

    /// Network preset (limits copied from midnight-node's
    /// res/<network>/ledger-parameters-config.json). See --list-networks.
    #[arg(short, long, default_value = limits::DEFAULT_NETWORK)]
    network: String,

    /// Limits file overriding --network. Accepts a node
    /// ledger-parameters-config.json from any node release (optionally with
    /// a top-level `weight` object), a bare node `limits` object, or the
    /// legacy deploy-check 0.1 flat format.
    #[arg(long, value_name = "FILE")]
    limits_file: Option<PathBuf>,

    /// List the embedded network presets and exit.
    #[arg(long)]
    list_networks: bool,

    /// Use this bytes_written instead of computing one, e.g. the figure a
    /// chain or the ledger's `Transaction.cost()` reported for a deploy.
    #[arg(long, value_name = "BYTES")]
    bytes_written: Option<u64>,

    /// Override the block weight (Substrate max block ref_time), nanoseconds.
    #[arg(long, value_name = "NS")]
    block_weight_ns: Option<u64>,

    /// Override the normal dispatch ratio, percent.
    #[arg(long, value_name = "PERCENT")]
    normal_dispatch_percent: Option<u32>,

    /// Override the block-initialization allowance, percent of the block.
    #[arg(long, value_name = "PERCENT")]
    block_init_percent: Option<u32>,

    /// Override the base extrinsic weight, nanoseconds.
    #[arg(long, value_name = "NS")]
    base_extrinsic_ns: Option<u64>,

    /// Override the flat per-transaction weight (midnight.setTxSizeWeight),
    /// nanoseconds.
    #[arg(long, value_name = "NS")]
    flat_tx_weight_ns: Option<u64>,

    /// Output report as JSON instead of human-readable.
    #[arg(long)]
    json: bool,

    /// Compute exact bytes_written with a linked ledger's cost model.
    /// Requires a binary built with `--features exact-ledger8` (alias
    /// `exact`) or `--features exact-ledger9`.
    #[arg(long)]
    exact: bool,

    /// Ledger backend for --exact when more than one is compiled in.
    #[arg(long, value_name = "VERSION")]
    ledger: Option<exact::LedgerVersion>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.list_networks {
        for n in limits::network_names() {
            println!("{n}");
        }
        return Ok(());
    }
    let build_dir = cli.build_dir.clone().expect("required by clap");

    let mut limits = match &cli.limits_file {
        Some(p) => limits::load_from_file(p)
            .with_context(|| format!("loading limits from {}", p.display()))?,
        None => limits::for_network(&cli.network)?,
    };
    let ns = |v: u64| v.saturating_mul(1_000);
    if let Some(v) = cli.block_weight_ns {
        limits.weight.block_weight_ps = ns(v);
    }
    if let Some(v) = cli.normal_dispatch_percent {
        limits.weight.normal_dispatch_percent = v;
    }
    if let Some(v) = cli.block_init_percent {
        limits.weight.block_init_percent = v;
    }
    if let Some(v) = cli.base_extrinsic_ns {
        limits.weight.base_extrinsic_ps = ns(v);
    }
    if let Some(v) = cli.flat_tx_weight_ns {
        limits.weight.flat_tx_weight_ps = ns(v);
    }

    let artifacts = artifacts::load(&build_dir)
        .with_context(|| format!("reading build dir {}", build_dir.display()))?;

    let exact_bytes_written = if cli.exact {
        let (_version, cost) =
            exact::compute(&artifacts, cli.ledger).context("computing exact bytes_written")?;
        Some(cost.bytes_written)
    } else {
        None
    };

    let report = report::evaluate(&artifacts, &limits, exact_bytes_written, cli.bytes_written);

    if cli.json {
        serde_json::to_writer_pretty(std::io::stdout(), &report)?;
        println!();
    } else {
        report.print_human(&mut std::io::stdout())?;
    }

    if report.fits() {
        Ok(())
    } else {
        std::process::exit(1);
    }
}
