//! Headless CLI for batch Monte Carlo simulation with Faultline.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use tracing::{error, info};

use faultline_engine::{Engine, validate_scenario};
use faultline_stats::compute_summary;
use faultline_types::scenario::Scenario;
use faultline_types::stats::{MonteCarloResult, RunResult};

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

/// Faultline — conflict simulation engine.
#[derive(Parser, Debug)]
#[command(name = "faultline", about = "Monte Carlo conflict simulation", version)]
struct Cli {
    /// Path to scenario TOML file.
    #[arg(value_name = "SCENARIO")]
    scenario: PathBuf,

    /// Number of Monte Carlo runs.
    #[arg(short = 'n', long = "runs", default_value_t = 1000)]
    runs: u32,

    /// Base RNG seed (random if not specified).
    #[arg(short = 's', long = "seed")]
    seed: Option<u64>,

    /// Output directory.
    #[arg(short = 'o', long = "output", default_value = "./output")]
    output: PathBuf,

    /// Output format: json, csv, both.
    #[arg(short = 'f', long = "format", default_value = "both")]
    format: OutputFormat,

    /// Parallel workers (defaults to number of CPUs).
    #[arg(short = 'j', long = "jobs")]
    jobs: Option<usize>,

    /// Run a single simulation with replay snapshots.
    #[arg(long = "single-run")]
    single_run: bool,

    /// Run sensitivity analysis (placeholder).
    #[arg(long = "sensitivity")]
    sensitivity: bool,

    /// Suppress progress output.
    #[arg(long = "quiet")]
    quiet: bool,

    /// Validate scenario file without running.
    #[arg(long = "validate")]
    validate: bool,

    /// Verbose logging.
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
}

/// Supported output formats.
#[derive(Clone, Debug, clap::ValueEnum)]
enum OutputFormat {
    Json,
    Csv,
    Both,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up tracing.
    let max_level = if cli.verbose {
        tracing::Level::DEBUG
    } else if cli.quiet {
        tracing::Level::ERROR
    } else {
        tracing::Level::INFO
    };
    tracing_subscriber::fmt()
        .with_max_level(max_level)
        .with_target(false)
        .init();

    // Load scenario.
    let toml_str = fs::read_to_string(&cli.scenario)
        .with_context(|| format!("failed to read scenario file: {}", cli.scenario.display()))?;

    let scenario: Scenario =
        toml::from_str(&toml_str).with_context(|| "failed to parse scenario TOML")?;

    // Validate.
    validate_scenario(&scenario).with_context(|| "scenario validation failed")?;

    if cli.validate {
        info!("scenario is valid");
        return Ok(());
    }

    // Ensure output directory exists.
    fs::create_dir_all(&cli.output).with_context(|| {
        format!(
            "failed to create output directory: {}",
            cli.output.display()
        )
    })?;

    if cli.single_run {
        return run_single(&cli, &scenario);
    }

    if cli.sensitivity {
        info!("sensitivity analysis is not yet implemented");
        return Ok(());
    }

    // Monte Carlo run.
    run_monte_carlo(&cli, &scenario)
}

// ---------------------------------------------------------------------------
// Single run
// ---------------------------------------------------------------------------

fn run_single(cli: &Cli, scenario: &Scenario) -> Result<()> {
    let seed = cli
        .seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));

    info!(seed, "running single simulation");

    let mut engine =
        Engine::with_seed(scenario.clone(), seed).with_context(|| "failed to create engine")?;

    let result = engine.run().with_context(|| "engine run failed")?;

    info!(
        final_tick = result.final_tick,
        victor = ?result.outcome.victor,
        "simulation complete"
    );

    write_result_json(cli, &result)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Monte Carlo
// ---------------------------------------------------------------------------

fn run_monte_carlo(cli: &Cli, scenario: &Scenario) -> Result<()> {
    let base_seed = cli
        .seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));

    let num_jobs = cli.jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    });

    info!(
        runs = cli.runs,
        jobs = num_jobs,
        base_seed,
        "starting Monte Carlo simulation"
    );

    // Configure rayon thread pool.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_jobs)
        .build()
        .with_context(|| "failed to build rayon thread pool")?;

    let scenario_clone = scenario.clone();
    let num_runs = cli.runs;

    let runs: Vec<RunResult> = pool.install(|| {
        (0..num_runs)
            .into_par_iter()
            .map(|i| {
                let seed = base_seed.wrapping_add(u64::from(i));
                let mut engine = match Engine::with_seed(scenario_clone.clone(), seed) {
                    Ok(e) => e,
                    Err(e) => {
                        error!(run_index = i, "engine creation failed: {e}");
                        // Return a dummy result to avoid panicking
                        // inside the parallel iterator.
                        return RunResult {
                            run_index: i,
                            seed,
                            outcome: faultline_types::stats::Outcome {
                                victor: None,
                                victory_condition: None,
                                final_tension: 0.0,
                            },
                            final_tick: 0,
                            snapshots: Vec::new(),
                        };
                    },
                };

                match engine.run() {
                    Ok(mut result) => {
                        result.run_index = i;
                        result.seed = seed;
                        result
                    },
                    Err(e) => {
                        error!(run_index = i, "engine run failed: {e}");
                        RunResult {
                            run_index: i,
                            seed,
                            outcome: faultline_types::stats::Outcome {
                                victor: None,
                                victory_condition: None,
                                final_tension: 0.0,
                            },
                            final_tick: 0,
                            snapshots: Vec::new(),
                        }
                    },
                }
            })
            .collect()
    });

    let summary = compute_summary(&runs, scenario);
    let mc_result = MonteCarloResult { runs, summary };

    info!(
        total_runs = mc_result.summary.total_runs,
        avg_duration = mc_result.summary.average_duration,
        "Monte Carlo complete"
    );

    // Report win rates.
    for (fid, rate) in &mc_result.summary.win_rates {
        info!(faction = %fid, win_rate = rate, "faction win rate");
    }

    write_outputs(cli, &mc_result)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

fn write_result_json(cli: &Cli, result: &RunResult) -> Result<()> {
    let path = cli.output.join("single_run.json");
    let json =
        serde_json::to_string_pretty(result).with_context(|| "failed to serialize run result")?;
    fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    info!(path = %path.display(), "wrote single run result");
    Ok(())
}

fn write_outputs(cli: &Cli, result: &MonteCarloResult) -> Result<()> {
    match cli.format {
        OutputFormat::Json | OutputFormat::Both => {
            write_json_summary(cli, result)?;
        },
        OutputFormat::Csv => {},
    }

    match cli.format {
        OutputFormat::Csv | OutputFormat::Both => {
            write_csv_summary(cli, result)?;
        },
        OutputFormat::Json => {},
    }

    Ok(())
}

fn write_json_summary(cli: &Cli, result: &MonteCarloResult) -> Result<()> {
    let path = cli.output.join("summary.json");
    let json = serde_json::to_string_pretty(&result.summary)
        .with_context(|| "failed to serialize summary")?;
    fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    info!(path = %path.display(), "wrote JSON summary");
    Ok(())
}

fn write_csv_summary(cli: &Cli, result: &MonteCarloResult) -> Result<()> {
    let path = cli.output.join("runs.csv");

    let mut lines = Vec::with_capacity(result.runs.len() + 1);
    lines.push("run_index,seed,victor,victory_condition,final_tick,final_tension".to_string());

    for run in &result.runs {
        let victor = run
            .outcome
            .victor
            .as_ref()
            .map_or("none".to_string(), |v| v.to_string());
        let vc = run.outcome.victory_condition.as_deref().unwrap_or("none");
        lines.push(format!(
            "{},{},{},{},{},{}",
            run.run_index, run.seed, victor, vc, run.final_tick, run.outcome.final_tension,
        ));
    }

    fs::write(&path, lines.join("\n"))
        .with_context(|| format!("failed to write {}", path.display()))?;
    info!(path = %path.display(), "wrote CSV runs");
    Ok(())
}
