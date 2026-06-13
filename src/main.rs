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
#[cfg(feature = "exact")]
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
    build_dir: PathBuf,

    /// Network preset. Determines the TransactionLimits applied.
    #[arg(short, long, default_value = "mainnet")]
    network: limits::NetworkPreset,

    /// Optional JSON file overriding TransactionLimits. Takes precedence over --network.
    #[arg(long, value_name = "FILE")]
    limits_file: Option<PathBuf>,

    /// Output report as JSON instead of human-readable.
    #[arg(long)]
    json: bool,

    /// Use exact `bytes_written` via midnight-ledger `cost_model.tree_copy`.
    /// Requires the binary to be built with `--features exact`. Without
    /// the feature, this flag returns an error explaining how to rebuild.
    #[arg(long)]
    exact: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let limits = match &cli.limits_file {
        Some(p) => limits::load_from_file(p)
            .with_context(|| format!("loading limits from {}", p.display()))?,
        None => cli.network.limits(),
    };

    let artifacts = artifacts::load(&cli.build_dir)
        .with_context(|| format!("reading build dir {}", cli.build_dir.display()))?;

    let exact_bytes_written = if cli.exact {
        #[cfg(feature = "exact")]
        {
            Some(
                exact::compute(&artifacts)
                    .context("computing exact bytes_written via cost_model.tree_copy")?
                    .bytes_written,
            )
        }
        #[cfg(not(feature = "exact"))]
        {
            anyhow::bail!(
                "--exact requires the `exact` cargo feature. Rebuild with:\n  cargo install --git https://github.com/verifiably-human/deploy-check --features exact\nor for a local checkout:\n  cargo build --release --features exact"
            );
        }
    } else {
        None
    };

    let report = report::evaluate(&artifacts, &limits, exact_bytes_written);

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
