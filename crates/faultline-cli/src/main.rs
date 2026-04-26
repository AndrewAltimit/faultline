//! Headless CLI for batch Monte Carlo simulation with Faultline.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use tracing::{error, info};

use faultline_engine::{Engine, validate_scenario};
use faultline_stats::compute_summary;
use faultline_stats::counterfactual::{ComparisonReport, ParamOverride};
use faultline_stats::manifest::{self, ManifestMcConfig, ManifestMode, RunManifest, VerifyResult};
use faultline_stats::search::{SearchConfig, SearchMethod, SearchResult};
use faultline_types::migration::{
    self, CURRENT_SCHEMA_VERSION, LoadedScenario, load_scenario_str, migrate_scenario_str,
};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{MonteCarloConfig, MonteCarloResult, RunResult};
use faultline_types::strategy_space::SearchObjective;

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
    ///
    /// Mutually exclusive with the other analysis modes.
    #[arg(
        long = "single-run",
        conflicts_with_all = ["sensitivity", "counterfactual", "compare"]
    )]
    single_run: bool,

    /// Run sensitivity analysis on a parameter.
    ///
    /// Mutually exclusive with `--counterfactual` and `--compare`.
    #[arg(long = "sensitivity", conflicts_with_all = ["counterfactual", "compare"])]
    sensitivity: bool,

    /// Parameter path for sensitivity analysis (e.g. "faction.gov.initial_morale").
    #[arg(long = "sensitivity-param", requires = "sensitivity")]
    sensitivity_param: Option<String>,

    /// Sensitivity range as "low:high:steps" (e.g. "0.2:1.0:5").
    #[arg(
        long = "sensitivity-range",
        default_value = "0.1:0.9:5",
        requires = "sensitivity"
    )]
    sensitivity_range: String,

    /// Number of Monte Carlo runs per sensitivity step.
    #[arg(
        long = "sensitivity-runs",
        default_value_t = 100,
        requires = "sensitivity"
    )]
    sensitivity_runs: u32,

    /// Counterfactual override of the form `<param.path>=<value>`.
    ///
    /// Pass repeatedly to stack overrides. The baseline scenario is
    /// run first, then the overridden variant is run with the same
    /// seed and run count so the reported deltas isolate the
    /// parameter change. Supported paths are documented in
    /// `faultline_stats::sensitivity::get_param`.
    ///
    /// Mutually exclusive with `--compare` — to evaluate a
    /// counterfactual against an alternative scenario, run two
    /// separate `--counterfactual` invocations and diff the JSON
    /// outputs.
    ///
    /// Note: `--jobs` is currently ignored in this mode; both batches
    /// run sequentially via `MonteCarloRunner::run` to keep delta
    /// determinism trivially auditable. Plain Monte Carlo runs (no
    /// `--counterfactual` / `--compare`) still parallelise via rayon.
    #[arg(
        long = "counterfactual",
        value_name = "PATH=VALUE",
        conflicts_with = "compare"
    )]
    counterfactual: Vec<String>,

    /// Path to a second scenario TOML for side-by-side comparison.
    ///
    /// Runs baseline and alt scenarios with matching seed / run
    /// count; output includes a comparison report with per-faction
    /// win rate deltas and per-chain feasibility deltas.
    ///
    /// Note: `--jobs` is currently ignored in this mode; both batches
    /// run sequentially. See `--counterfactual` for the rationale.
    #[arg(long = "compare", value_name = "OTHER_SCENARIO")]
    compare: Option<PathBuf>,

    /// Run strategy search (Epic H).
    ///
    /// Reads the scenario's `[strategy_space]` declaration, samples
    /// trial assignments according to `--search-method`, evaluates each
    /// via Monte Carlo, and emits a search report identifying the
    /// best-by-objective trial and the non-dominated Pareto frontier.
    ///
    /// Mutually exclusive with the other run modes (`--single-run`,
    /// `--sensitivity`, `--counterfactual`, `--compare`, `--verify`,
    /// `--validate`, `--migrate`). Use `--search-objective` repeatedly
    /// to evaluate multiple objectives; the Pareto frontier spans all
    /// of them.
    #[arg(
        long = "search",
        conflicts_with_all = [
            "single_run", "sensitivity", "counterfactual",
            "compare", "verify", "validate", "migrate"
        ]
    )]
    search: bool,

    /// Sampling method for `--search`. `random` draws uniform
    /// assignments from each variable's domain (count = `--search-trials`).
    /// `grid` enumerates the Cartesian product of per-variable level
    /// sets, truncated to `--search-trials`.
    #[arg(
        long = "search-method",
        value_name = "METHOD",
        default_value = "random",
        requires = "search"
    )]
    search_method: CliSearchMethod,

    /// Number of search trials. Each trial runs an independent inner
    /// Monte Carlo batch sized by `--search-runs`.
    #[arg(long = "search-trials", default_value_t = 32, requires = "search")]
    search_trials: u32,

    /// Inner Monte Carlo run count for each search trial. Smaller
    /// values give noisier objective estimates but let an analyst
    /// explore more of the strategy space within a fixed compute
    /// budget. Defaults to 100.
    #[arg(long = "search-runs", default_value_t = 100, requires = "search")]
    search_runs: u32,

    /// Search-only RNG seed. Independent of `--seed` (the inner Monte
    /// Carlo seed). Re-using the same `--search-seed` reproduces the
    /// trial assignments; re-using the same `--seed` reproduces each
    /// trial's evaluation.
    #[arg(long = "search-seed", requires = "search")]
    search_seed: Option<u64>,

    /// Search objective. Pass repeatedly to declare multi-objective
    /// search. Format: `<metric>` or `<metric>:<argument>`. Supported
    /// metrics: `maximize_win_rate:<faction>`, `minimize_detection`,
    /// `minimize_attacker_cost`, `maximize_cost_asymmetry`,
    /// `minimize_duration`. When omitted, the runner falls back to the
    /// scenario's `[strategy_space].objectives` (if present); if both
    /// are empty the run fails with a clear error.
    #[arg(
        long = "search-objective",
        value_name = "OBJECTIVE",
        requires = "search"
    )]
    search_objective: Vec<String>,

    /// Verify a saved run by replaying it from a manifest.
    ///
    /// Loads the manifest from `<MANIFEST_PATH>`, hashes the
    /// positional scenario file, asserts it matches the manifest's
    /// `scenario_hash`, then replays the recorded mode + Monte Carlo
    /// config and compares the freshly computed output hash to the
    /// saved one. Exits non-zero on mismatch with a structured diff.
    ///
    /// The manifest's `mc_config` overrides `--runs` and `--seed`;
    /// the recorded `mode` overrides the run-mode flags. So in a
    /// verify invocation the CLI's run-mode flags are ignored — only
    /// the scenario path, the output directory, and `--verify` itself
    /// are consulted.
    #[arg(
        long = "verify",
        value_name = "MANIFEST_PATH",
        conflicts_with_all = ["single_run", "sensitivity", "counterfactual", "compare", "validate"]
    )]
    verify: Option<PathBuf>,

    /// Suppress progress output.
    #[arg(long = "quiet")]
    quiet: bool,

    /// Validate scenario file without running.
    #[arg(long = "validate")]
    validate: bool,

    /// Run schema migrations on the scenario and emit the upgraded TOML.
    ///
    /// Loads the scenario, advances `meta.schema_version` from
    /// whatever was authored to the current version, validates the
    /// result, and prints the upgraded TOML to stdout. Combine with
    /// `--in-place` to overwrite the source file. Mutually exclusive
    /// with the run modes — migrate is a pure schema operation that
    /// does not start the engine.
    ///
    /// Caveat: emitted TOML is the canonical form (BTreeMap-sorted
    /// keys, single-line strings, no comments). Authorial formatting
    /// is not preserved. For scenarios where formatting matters, diff
    /// the migrated form against the source and apply changes by
    /// hand rather than using `--in-place`.
    #[arg(
        long = "migrate",
        conflicts_with_all = [
            "single_run", "sensitivity", "counterfactual",
            "compare", "verify", "validate"
        ]
    )]
    migrate: bool,

    /// With `--migrate`, overwrite the source scenario file in place
    /// instead of printing to stdout.
    #[arg(long = "in-place", requires = "migrate")]
    in_place: bool,

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

/// CLI form of `faultline_stats::search::SearchMethod` so clap can
/// parse it without depending on `clap` from the stats crate.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum CliSearchMethod {
    Random,
    Grid,
}

impl From<CliSearchMethod> for SearchMethod {
    fn from(m: CliSearchMethod) -> Self {
        match m {
            CliSearchMethod::Random => SearchMethod::Random,
            CliSearchMethod::Grid => SearchMethod::Grid,
        }
    }
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

    // `--migrate` is a pure schema operation: read TOML, advance the
    // version, write TOML back out. We short-circuit before validation
    // because a migration's whole job is to make a stale scenario
    // valid — running validation first would refuse the scenario for
    // the very reason migration would fix.
    if cli.migrate {
        return run_migrate(&cli, &toml_str);
    }

    let LoadedScenario {
        scenario,
        source_version,
        migrated,
    } = load_scenario_str(&toml_str)
        .with_context(|| format!("failed to load scenario {}", cli.scenario.display()))?;

    if migrated {
        // Surface the silent in-memory upgrade so an analyst notices
        // their on-disk fixture is stale instead of finding out later
        // when its hash drifts.
        tracing::warn!(
            scenario = %cli.scenario.display(),
            source_version,
            current_version = CURRENT_SCHEMA_VERSION,
            "scenario was authored against an older schema; migrating in memory. Run `faultline {scenario_path} --migrate --in-place` to persist the upgraded form.",
            scenario_path = cli.scenario.display(),
        );
    }

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

    if let Some(ref manifest_path) = cli.verify {
        return run_verify(&cli, &scenario, manifest_path);
    }

    if cli.single_run {
        return run_single(&cli, &scenario);
    }

    if cli.sensitivity {
        return run_sensitivity_analysis(&cli, &scenario);
    }

    if !cli.counterfactual.is_empty() {
        return run_counterfactual_analysis(&cli, &scenario);
    }

    if let Some(ref alt_path) = cli.compare {
        return run_compare_analysis(&cli, &scenario, alt_path);
    }

    if cli.search {
        return run_search_analysis(&cli, &scenario);
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

    let result = execute_single(scenario, seed)?;

    write_result_json(cli, &result)?;

    let manifest_mc = ManifestMcConfig {
        num_runs: 1,
        base_seed: seed,
        // `--single-run` always collects snapshots; the manifest field
        // exists for parity with the MC modes' config.
        collect_snapshots: true,
    };
    let mode = ManifestMode::SingleRun;
    let output_hash =
        manifest::output_hash(&result).with_context(|| "failed to hash single-run result")?;
    let manifest_obj = build_manifest_object(cli, scenario, manifest_mc, mode, output_hash)?;
    write_manifest_object(cli, &manifest_obj)?;

    Ok(())
}

fn execute_single(scenario: &Scenario, seed: u64) -> Result<RunResult> {
    info!(seed, "running single simulation");

    let mut engine =
        Engine::with_seed(scenario.clone(), seed).with_context(|| "failed to create engine")?;

    let result = engine.run().with_context(|| "engine run failed")?;

    info!(
        final_tick = result.final_tick,
        victor = ?result.outcome.victor,
        "simulation complete"
    );

    Ok(result)
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

    let mc_result = execute_monte_carlo(scenario, base_seed, cli.runs, num_jobs)?;

    info!(
        total_runs = mc_result.summary.total_runs,
        avg_duration = mc_result.summary.average_duration,
        "Monte Carlo complete"
    );

    // Report win rates.
    for (fid, rate) in &mc_result.summary.win_rates {
        info!(faction = %fid, win_rate = rate, "faction win rate");
    }

    let manifest_mc = ManifestMcConfig {
        num_runs: cli.runs,
        base_seed,
        collect_snapshots: false,
    };
    let mode = ManifestMode::MonteCarlo;
    let output_hash =
        manifest::summary_hash(&mc_result.summary).with_context(|| "failed to hash MC summary")?;
    let manifest_obj = build_manifest_object(cli, scenario, manifest_mc, mode, output_hash)?;

    write_outputs(cli, &mc_result, scenario, Some(&manifest_obj))?;
    write_manifest_object(cli, &manifest_obj)?;

    Ok(())
}

/// Execute a parallel Monte Carlo batch and return the aggregated result.
///
/// Failed runs are logged and skipped (matching the historical CLI
/// behaviour); the resulting summary is computed over whichever runs
/// completed. This is deterministic given a fixed seed because rayon's
/// indexed `into_par_iter` over a `Range` collects in source order, and
/// engine failures are themselves a function of the per-run seed.
fn execute_monte_carlo(
    scenario: &Scenario,
    base_seed: u64,
    num_runs: u32,
    num_jobs: usize,
) -> Result<MonteCarloResult> {
    info!(
        runs = num_runs,
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
    let failed_runs = std::sync::atomic::AtomicU32::new(0);

    let runs: Vec<RunResult> = pool.install(|| {
        (0..num_runs)
            .into_par_iter()
            .filter_map(|i| {
                let seed = base_seed.wrapping_add(u64::from(i));
                let mut engine = match Engine::with_seed(scenario_clone.clone(), seed) {
                    Ok(e) => e,
                    Err(e) => {
                        error!(run_index = i, "engine creation failed: {e}");
                        failed_runs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return None;
                    },
                };

                match engine.run() {
                    Ok(mut result) => {
                        result.run_index = i;
                        result.seed = seed;
                        Some(result)
                    },
                    Err(e) => {
                        error!(run_index = i, "engine run failed: {e}");
                        failed_runs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        None
                    },
                }
            })
            .collect()
    });

    let num_failed = failed_runs.load(std::sync::atomic::Ordering::Relaxed);
    if num_failed > 0 {
        tracing::warn!(
            failed = num_failed,
            succeeded = runs.len(),
            "some Monte Carlo runs failed"
        );
    }

    let summary = compute_summary(&runs, scenario);
    Ok(MonteCarloResult { runs, summary })
}

// ---------------------------------------------------------------------------
// Counterfactual & comparison (Epic B)
// ---------------------------------------------------------------------------

fn run_counterfactual_analysis(cli: &Cli, scenario: &Scenario) -> Result<()> {
    let overrides: Vec<ParamOverride> = cli
        .counterfactual
        .iter()
        .map(|s| ParamOverride::parse(s))
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| "failed to parse --counterfactual override")?;

    let base_seed = cli
        .seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));

    let config = MonteCarloConfig {
        num_runs: cli.runs,
        seed: Some(base_seed),
        collect_snapshots: false,
        parallel: false,
    };

    info!(
        runs = cli.runs,
        base_seed,
        overrides = overrides.len(),
        "starting counterfactual analysis"
    );

    let report = faultline_stats::counterfactual::run_counterfactual(scenario, &config, &overrides)
        .with_context(|| "counterfactual analysis failed")?;

    let manifest_mc = ManifestMcConfig::from_config(&config, base_seed);
    let mode = ManifestMode::Counterfactual {
        // Store the raw `path=value` strings — the same form the user
        // would type to reproduce. Reparsing happens in verify.
        overrides: cli.counterfactual.clone(),
    };
    let output_hash =
        manifest::output_hash(&report).with_context(|| "failed to hash comparison report")?;
    let manifest_obj = build_manifest_object(cli, scenario, manifest_mc, mode, output_hash)?;

    write_comparison_outputs(cli, &report, scenario, Some(&manifest_obj))?;
    write_manifest_object(cli, &manifest_obj)?;

    Ok(())
}

fn run_compare_analysis(cli: &Cli, scenario: &Scenario, alt_path: &Path) -> Result<()> {
    let alt_toml = fs::read_to_string(alt_path)
        .with_context(|| format!("failed to read --compare scenario: {}", alt_path.display()))?;
    let LoadedScenario {
        scenario: alt_scenario,
        source_version: alt_source_version,
        migrated: alt_migrated,
    } = load_scenario_str(&alt_toml)
        .with_context(|| format!("failed to load --compare scenario: {}", alt_path.display()))?;
    if alt_migrated {
        tracing::warn!(
            scenario = %alt_path.display(),
            source_version = alt_source_version,
            current_version = CURRENT_SCHEMA_VERSION,
            "--compare scenario was authored against an older schema; migrating in memory."
        );
    }
    validate_scenario(&alt_scenario).with_context(|| "--compare scenario validation failed")?;

    let base_seed = cli
        .seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));

    let config = MonteCarloConfig {
        num_runs: cli.runs,
        seed: Some(base_seed),
        collect_snapshots: false,
        parallel: false,
    };

    let alt_label = alt_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("alt")
        .to_string();

    info!(
        runs = cli.runs,
        base_seed,
        alt_label = %alt_label,
        "starting comparison analysis"
    );

    let report =
        faultline_stats::counterfactual::run_compare(scenario, &alt_scenario, &alt_label, &config)
            .with_context(|| "scenario comparison failed")?;

    let manifest_mc = ManifestMcConfig::from_config(&config, base_seed);
    let alt_hash =
        manifest::scenario_hash(&alt_scenario).with_context(|| "failed to hash alt scenario")?;
    let mode = ManifestMode::Compare {
        alt_scenario_path: alt_path.display().to_string(),
        alt_scenario_hash: alt_hash,
    };
    let output_hash =
        manifest::output_hash(&report).with_context(|| "failed to hash comparison report")?;
    let manifest_obj = build_manifest_object(cli, scenario, manifest_mc, mode, output_hash)?;

    write_comparison_outputs(cli, &report, scenario, Some(&manifest_obj))?;
    write_manifest_object(cli, &manifest_obj)?;

    Ok(())
}

fn write_comparison_outputs(
    cli: &Cli,
    report: &ComparisonReport,
    scenario: &Scenario,
    manifest_obj: Option<&RunManifest>,
) -> Result<()> {
    // Comparison mode produces a *delta* between two Monte Carlo batches.
    // The per-run CSV shape (one row per simulation) does not apply, so
    // the two artifacts are JSON (the structured delta) and Markdown
    // (the rendered analyst report). `--format` selects between them:
    //   - `json` → only `comparison.json`
    //   - `csv`  → CSV does not apply here, so we emit both as a fallback
    //              and warn that `--format csv` is a no-op for comparisons
    //   - `both` → emit both (default behaviour)
    //
    // JSON is the canonical structured artifact for comparison output and
    // is emitted unconditionally for every current `OutputFormat`. We use
    // an exhaustive `match` so adding a new variant forces a deliberate
    // decision here rather than silently producing no output.
    let (want_json, want_md) = match cli.format {
        OutputFormat::Json => (true, false),
        OutputFormat::Both => (true, true),
        OutputFormat::Csv => (true, true),
    };

    if matches!(cli.format, OutputFormat::Csv) {
        tracing::warn!(
            "--format csv is not meaningful for comparison output (per-run CSV shape doesn't apply to a delta); falling back to JSON + Markdown"
        );
    }

    if want_json {
        let json_path = cli.output.join("comparison.json");
        let json = serde_json::to_string_pretty(report)
            .with_context(|| "failed to serialize comparison report")?;
        fs::write(&json_path, json)
            .with_context(|| format!("failed to write {}", json_path.display()))?;
        info!(path = %json_path.display(), "wrote comparison JSON");
    }

    if want_md {
        let md_path = cli.output.join("comparison_report.md");
        let body = faultline_stats::report::render_comparison_markdown(report, scenario);
        let md = with_manifest_front_matter(&body, manifest_obj);
        fs::write(&md_path, md)
            .with_context(|| format!("failed to write {}", md_path.display()))?;
        info!(path = %md_path.display(), "wrote comparison Markdown report");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Sensitivity analysis
// ---------------------------------------------------------------------------

fn run_sensitivity_analysis(cli: &Cli, scenario: &Scenario) -> Result<()> {
    let param = cli.sensitivity_param.as_deref().ok_or_else(|| {
        anyhow::anyhow!("--sensitivity-param is required for sensitivity analysis")
    })?;

    let (low, high, steps) = parse_sensitivity_range(&cli.sensitivity_range)?;

    let base_seed = cli
        .seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));

    let config = faultline_types::stats::MonteCarloConfig {
        num_runs: cli.sensitivity_runs,
        seed: Some(base_seed),
        collect_snapshots: false,
        parallel: false,
    };

    info!(
        param,
        low,
        high,
        steps,
        runs_per_step = cli.sensitivity_runs,
        "starting sensitivity analysis"
    );

    let result =
        faultline_stats::sensitivity::run_sensitivity(scenario, &config, param, low, high, steps)
            .with_context(|| "sensitivity analysis failed")?;

    info!(
        param = result.parameter,
        baseline = result.baseline_value,
        steps = result.varied_values.len(),
        "sensitivity analysis complete"
    );

    write_sensitivity_output(cli, &result)?;

    let manifest_mc = ManifestMcConfig::from_config(&config, base_seed);
    let mode = ManifestMode::Sensitivity {
        param: param.to_string(),
        low,
        high,
        steps,
        runs_per_step: cli.sensitivity_runs,
    };
    let output_hash =
        manifest::output_hash(&result).with_context(|| "failed to hash sensitivity result")?;
    let manifest_obj = build_manifest_object(cli, scenario, manifest_mc, mode, output_hash)?;
    write_manifest_object(cli, &manifest_obj)?;

    Ok(())
}

fn parse_sensitivity_range(range: &str) -> Result<(f64, f64, u32)> {
    let parts: Vec<&str> = range.split(':').collect();
    if parts.len() != 3 {
        anyhow::bail!("sensitivity range must be 'low:high:steps', got '{range}'");
    }
    let low: f64 = parts[0]
        .parse()
        .with_context(|| format!("invalid low value: '{}'", parts[0]))?;
    let high: f64 = parts[1]
        .parse()
        .with_context(|| format!("invalid high value: '{}'", parts[1]))?;
    let steps: u32 = parts[2]
        .parse()
        .with_context(|| format!("invalid steps value: '{}'", parts[2]))?;
    Ok((low, high, steps))
}

fn write_sensitivity_output(
    cli: &Cli,
    result: &faultline_types::stats::SensitivityResult,
) -> Result<()> {
    // JSON output.
    let json_path = cli.output.join("sensitivity.json");
    let json = serde_json::to_string_pretty(result)
        .with_context(|| "failed to serialize sensitivity result")?;
    fs::write(&json_path, json)
        .with_context(|| format!("failed to write {}", json_path.display()))?;
    info!(path = %json_path.display(), "wrote sensitivity JSON");

    // CSV summary: one row per step with key metrics.
    let csv_path = cli.output.join("sensitivity.csv");
    let mut lines = Vec::with_capacity(result.varied_values.len() + 1);

    // Collect union of all faction IDs across all outcomes so factions
    // that only win in later steps still get a column.
    let all_factions: std::collections::BTreeSet<_> = result
        .outcomes
        .iter()
        .flat_map(|s| s.win_rates.keys().cloned())
        .collect();

    lines.push(format!(
        "parameter,value,avg_duration,stalemate_rate{}",
        all_factions
            .iter()
            .map(|fid| format!(",\"win_rate_{}\"", csv_escape(&fid.to_string())))
            .collect::<String>()
    ));

    for (i, summary) in result.outcomes.iter().enumerate() {
        let value = result.varied_values[i];
        let total_win_rate: f64 = summary.win_rates.values().sum();
        let stalemate_rate = 1.0 - total_win_rate;

        let mut line = format!(
            "\"{}\",{},{},{}",
            csv_escape(&result.parameter),
            value,
            summary.average_duration,
            stalemate_rate,
        );

        // Use the same faction order as the header (BTreeSet is sorted).
        for fid in &all_factions {
            let rate = summary.win_rates.get(fid).copied().unwrap_or(0.0);
            line.push_str(&format!(",{rate}"));
        }

        lines.push(line);
    }

    fs::write(&csv_path, lines.join("\n") + "\n")
        .with_context(|| format!("failed to write {}", csv_path.display()))?;
    info!(path = %csv_path.display(), "wrote sensitivity CSV");

    Ok(())
}

// ---------------------------------------------------------------------------
// Strategy search (Epic H)
// ---------------------------------------------------------------------------

fn run_search_analysis(cli: &Cli, scenario: &Scenario) -> Result<()> {
    // Objective resolution: CLI flags override the scenario's embedded
    // list. If both are empty the runner returns a clear error from
    // `run_search`, so we don't pre-emptively reject here — that keeps
    // the error message in one place.
    let objectives: Vec<SearchObjective> = if cli.search_objective.is_empty() {
        scenario.strategy_space.objectives.clone()
    } else {
        cli.search_objective
            .iter()
            .map(|s| SearchObjective::parse_cli(s).map_err(anyhow::Error::msg))
            .collect::<Result<Vec<_>>>()
            .with_context(|| "failed to parse --search-objective")?
    };

    let mc_seed = cli
        .seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));
    let search_seed = cli
        .search_seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));

    let mc_config = MonteCarloConfig {
        num_runs: cli.search_runs,
        seed: Some(mc_seed),
        collect_snapshots: false,
        parallel: false,
    };

    let method: SearchMethod = cli.search_method.into();
    let config = SearchConfig {
        trials: cli.search_trials,
        method,
        search_seed,
        mc_config,
        objectives: objectives.clone(),
    };

    info!(
        trials = cli.search_trials,
        method = ?method,
        search_seed,
        mc_seed,
        objectives = objectives.len(),
        "starting strategy search"
    );

    let result = faultline_stats::search::run_search(scenario, &config)
        .with_context(|| "strategy search failed")?;

    write_search_outputs(cli, &result)?;

    let manifest_mc = ManifestMcConfig::from_config(&config.mc_config, mc_seed);
    let mode = ManifestMode::Search {
        method,
        trials: cli.search_trials,
        search_seed,
        objectives: objectives.iter().map(SearchObjective::label).collect(),
    };
    let output_hash =
        manifest::output_hash(&result).with_context(|| "failed to hash search result")?;
    let manifest_obj = build_manifest_object(cli, scenario, manifest_mc, mode, output_hash)?;

    // Search has no per-trial CSV shape that maps cleanly (every trial
    // carries a full `MonteCarloSummary` — flattening that into rows
    // would lose information without a bespoke schema), so we always
    // emit the Markdown alongside the JSON regardless of `--format`.
    // `--format csv` is treated as "still emit JSON+MD"; CSV is a
    // no-op here. This matches the comparison-mode handling.
    if matches!(cli.format, OutputFormat::Csv) {
        tracing::warn!(
            "--format csv is not meaningful for strategy-search output \
             (per-trial CSV shape doesn't apply); falling back to JSON + Markdown"
        );
    }
    let md_path = cli.output.join("search_report.md");
    let body = faultline_stats::report::render_search_markdown(&result, scenario);
    let md = with_manifest_front_matter(&body, Some(&manifest_obj));
    fs::write(&md_path, md).with_context(|| format!("failed to write {}", md_path.display()))?;
    info!(path = %md_path.display(), "wrote search Markdown report");

    write_manifest_object(cli, &manifest_obj)?;
    Ok(())
}

fn write_search_outputs(cli: &Cli, result: &SearchResult) -> Result<()> {
    // Search emits a structured JSON artifact unconditionally — the
    // analyst's full record of what was tried and what each trial
    // scored. The Markdown is written separately in
    // `run_search_analysis` so the manifest header can land on top.
    let json_path = cli.output.join("search.json");
    let json = serde_json::to_string_pretty(result)
        .with_context(|| "failed to serialize search result")?;
    fs::write(&json_path, json)
        .with_context(|| format!("failed to write {}", json_path.display()))?;
    info!(path = %json_path.display(), "wrote search JSON");
    Ok(())
}

// ---------------------------------------------------------------------------
// Schema migration (Epic O)
// ---------------------------------------------------------------------------

/// `--migrate` mode: advance a scenario's `meta.schema_version` to
/// the current build's version. With `--in-place`, overwrite the
/// source file; otherwise print the migrated TOML to stdout. The
/// engine never starts — this is a pure schema operation, distinct
/// from `--validate` (which only checks shape).
///
/// The migrator is a no-op when the source is already at the current
/// version; for that case we still rewrite (or re-emit) so the
/// `meta.schema_version` field is explicitly present afterward — that
/// gives an analyst a single canonical form to commit and removes a
/// silent variant of "the scenario file disagrees with what the
/// engine actually loads."
///
/// Caveat: the emitted TOML is the canonical form of the parsed
/// scenario — keys are BTreeMap-sorted, multi-line strings get
/// collapsed, and comments are stripped. That's the cost of going
/// through the deserialize-then-reserialize migration pipeline. For
/// scenarios where formatting matters, run `--migrate` to a temp
/// file, diff against the source, and apply the changes by hand
/// instead of `--in-place`.
fn run_migrate(cli: &Cli, toml_str: &str) -> Result<()> {
    let value: toml::Value = toml::from_str(toml_str)
        .with_context(|| format!("failed to parse {}", cli.scenario.display()))?;
    let source_version = migration::extract_schema_version(&value)
        .with_context(|| "failed to read meta.schema_version")?;

    let migrated_toml = migrate_scenario_str(toml_str)
        .with_context(|| format!("failed to migrate scenario {}", cli.scenario.display()))?;

    // Sanity-check the migrated form by deserializing AND validating
    // before we touch stdout or disk. Two layers:
    //   1. Deserialize: catches a migration that produces structurally
    //      bad TOML (wrong field types, missing required fields).
    //   2. validate_scenario: catches a migration that produces a
    //      scenario the engine would refuse at run time (no factions,
    //      regions referencing missing borders, etc.).
    // Stdout-mode also runs both checks before emitting any bytes so
    // a redirect (`--migrate > new.toml`) never captures broken output.
    let migrated_scenario: Scenario = toml::from_str(&migrated_toml)
        .with_context(|| "migrated scenario failed to deserialize; refusing to emit")?;
    validate_scenario(&migrated_scenario).with_context(|| {
        "migrated scenario failed engine validation; refusing to emit. \
         The migration produced a structurally-valid TOML that the engine \
         would reject — fix the migration step or repair the source \
         scenario, then retry."
    })?;

    if cli.in_place {
        // Atomic in-place rewrite: write to a sibling temp file, then
        // `rename` (atomic on POSIX as long as both paths share a
        // filesystem). `fs::write` directly on the target would
        // truncate-then-write, so a kill mid-write would leave the
        // scenario file partially written with no recovery path.
        let mut tmp_os = cli.scenario.clone().into_os_string();
        tmp_os.push(".tmp");
        let tmp_path = PathBuf::from(tmp_os);
        fs::write(&tmp_path, &migrated_toml).with_context(|| {
            format!(
                "failed to write migrated scenario to temp file {}",
                tmp_path.display()
            )
        })?;
        if let Err(rename_err) = fs::rename(&tmp_path, &cli.scenario) {
            // Best-effort cleanup so a failed rename doesn't leave an
            // orphan `.tmp` next to the scenario. Ignore the cleanup
            // error — the rename failure is the real signal.
            let _ = fs::remove_file(&tmp_path);
            return Err(rename_err).with_context(|| {
                format!(
                    "failed to atomically replace {} with {}",
                    cli.scenario.display(),
                    tmp_path.display()
                )
            });
        }
        info!(
            scenario = %cli.scenario.display(),
            from = source_version,
            to = CURRENT_SCHEMA_VERSION,
            "scenario migrated in place"
        );
    } else {
        // Print to stdout so the user can pipe it: `faultline foo.toml
        // --migrate > foo-v2.toml`. We use print! (not info!) so the
        // bytes appear on stdout regardless of tracing log level. We
        // deliberately emit no trailing log line — tracing's default
        // formatter writes to stdout in this binary, so any info!()
        // here would append a non-TOML line into the redirected file
        // and break downstream parse. The captured TOML is the entire
        // user-facing output of the stdout-mode migrate.
        print!("{migrated_toml}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Manifest emission + verify (Epic Q)
// ---------------------------------------------------------------------------

/// Build a [`RunManifest`] from the run inputs without writing it to
/// disk. Splitting the build from the write lets the manifest hash
/// flow into the markdown report's front-matter before the manifest
/// itself is persisted.
fn build_manifest_object(
    cli: &Cli,
    scenario: &Scenario,
    mc_config: ManifestMcConfig,
    mode: ManifestMode,
    output_hash: String,
) -> Result<RunManifest> {
    let scenario_hash = manifest::scenario_hash(scenario)
        .with_context(|| "failed to hash scenario for manifest")?;
    manifest::build_manifest(
        cli.scenario.display().to_string(),
        scenario_hash,
        mc_config,
        mode,
        output_hash,
    )
    .with_context(|| "failed to build manifest")
}

/// Persist a manifest to `manifest.json` in the output directory.
fn write_manifest_object(cli: &Cli, manifest_obj: &RunManifest) -> Result<()> {
    let path = cli.output.join("manifest.json");
    let json = serde_json::to_string_pretty(manifest_obj)
        .with_context(|| "failed to serialize manifest")?;
    fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    info!(
        path = %path.display(),
        manifest_hash = %manifest_obj.manifest_hash,
        "wrote manifest"
    );
    Ok(())
}

/// Replay a saved manifest against the live scenario and assert
/// bit-identical output. Exits non-zero on any mismatch.
fn run_verify(cli: &Cli, scenario: &Scenario, manifest_path: &Path) -> Result<()> {
    let saved_str = fs::read_to_string(manifest_path)
        .with_context(|| format!("failed to read manifest: {}", manifest_path.display()))?;
    let saved: RunManifest = serde_json::from_str(&saved_str).with_context(|| {
        format!(
            "failed to parse manifest as JSON: {}",
            manifest_path.display()
        )
    })?;

    if saved.manifest_version != manifest::MANIFEST_VERSION {
        anyhow::bail!(
            "manifest version mismatch: saved={}, this build supports={}",
            saved.manifest_version,
            manifest::MANIFEST_VERSION
        );
    }

    // Self-integrity: re-derive the manifest's own hash before doing
    // any expensive replay work. Catches silent field tampering
    // (swapped `output_hash`, inflated `num_runs`) where the replay
    // would otherwise still match a manipulated `output_hash`.
    let recomputed_self_hash = manifest::compute_manifest_hash(&saved)
        .with_context(|| "failed to recompute manifest self-hash")?;
    if recomputed_self_hash != saved.manifest_hash {
        anyhow::bail!(
            "manifest self-hash mismatch:\n  recorded:   {}\n  recomputed: {}\nThe manifest file at {} has been altered after emission.",
            saved.manifest_hash,
            recomputed_self_hash,
            manifest_path.display(),
        );
    }

    info!(
        manifest_hash = %saved.manifest_hash,
        engine_version = %saved.engine_version,
        "verifying manifest"
    );

    // Hash the live scenario before doing any work — failing here saves
    // a multi-minute MC run on a hash that's already wrong.
    let live_scenario_hash =
        manifest::scenario_hash(scenario).with_context(|| "failed to hash live scenario")?;
    if live_scenario_hash != saved.scenario_hash {
        anyhow::bail!(
            "scenario hash mismatch:\n  saved:    {}\n  live:     {}\nThe scenario file at {} differs semantically from the one used to produce the manifest.",
            saved.scenario_hash,
            live_scenario_hash,
            cli.scenario.display(),
        );
    }
    info!("scenario hash matches; replaying run");

    let replay_manifest =
        replay_manifest_mode(cli, scenario, &saved).with_context(|| "manifest replay failed")?;

    match manifest::verify_manifest(&saved, &replay_manifest) {
        VerifyResult::Match => {
            info!(
                manifest_hash = %saved.manifest_hash,
                "verify OK: replay produced bit-identical output"
            );
            // Also print to stdout so a script consuming `faultline
            // verify` can grep for "VERIFY OK".
            println!(
                "VERIFY OK manifest_hash={} output_hash={}",
                saved.manifest_hash, saved.output_hash
            );
            Ok(())
        },
        VerifyResult::Mismatch { reason } => {
            error!(reason = %reason, "verify FAILED");
            anyhow::bail!("VERIFY FAILED: {reason}");
        },
    }
}

/// Re-execute the saved mode against `scenario` and produce the
/// manifest the replay would have emitted. The manifest's
/// `output_hash` is computed from the freshly produced output and is
/// the field that detects determinism drift.
fn replay_manifest_mode(
    cli: &Cli,
    scenario: &Scenario,
    saved: &RunManifest,
) -> Result<RunManifest> {
    let config = saved.mc_config.to_config();
    let scenario_path = cli.scenario.display().to_string();
    let scenario_hash = saved.scenario_hash.clone();

    let (mode, output_hash) = match &saved.mode {
        ManifestMode::SingleRun => {
            let result = execute_single(scenario, saved.mc_config.base_seed)?;
            let h = manifest::output_hash(&result)
                .with_context(|| "failed to hash single-run replay")?;
            (ManifestMode::SingleRun, h)
        },
        ManifestMode::MonteCarlo => {
            // Replay uses one job for stable ordering — the parallel
            // path is also deterministic but pinning to one thread
            // removes a degree of freedom from "why did the hash
            // mismatch?" debugging.
            let mc_result = execute_monte_carlo(
                scenario,
                saved.mc_config.base_seed,
                saved.mc_config.num_runs,
                1,
            )?;
            let h = manifest::summary_hash(&mc_result.summary)
                .with_context(|| "failed to hash MC replay summary")?;
            (ManifestMode::MonteCarlo, h)
        },
        ManifestMode::Counterfactual { overrides } => {
            let parsed: Vec<ParamOverride> = overrides
                .iter()
                .map(|s| ParamOverride::parse(s))
                .collect::<Result<Vec<_>, _>>()
                .with_context(|| "failed to reparse counterfactual overrides")?;
            let report =
                faultline_stats::counterfactual::run_counterfactual(scenario, &config, &parsed)
                    .with_context(|| "counterfactual replay failed")?;
            let h = manifest::output_hash(&report)
                .with_context(|| "failed to hash counterfactual replay")?;
            (
                ManifestMode::Counterfactual {
                    overrides: overrides.clone(),
                },
                h,
            )
        },
        ManifestMode::Compare {
            alt_scenario_path,
            alt_scenario_hash,
        } => {
            let alt_path = Path::new(alt_scenario_path);
            // Reject absolute paths and parent-traversal segments so a
            // crafted manifest cannot make verify read arbitrary files
            // (e.g. `/etc/passwd`, `../../secret.toml`). Legitimate
            // emissions always store a CWD-relative path that descends
            // into a scenario directory.
            if alt_path.is_absolute() {
                anyhow::bail!(
                    "alt scenario path in manifest is absolute, refusing to read for safety: {}",
                    alt_scenario_path
                );
            }
            if alt_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                anyhow::bail!(
                    "alt scenario path in manifest contains parent traversal, refusing to read for safety: {}",
                    alt_scenario_path
                );
            }
            let alt_toml = fs::read_to_string(alt_path)
                .with_context(|| format!("failed to read alt scenario: {}", alt_path.display()))?;
            let LoadedScenario {
                scenario: alt_scenario,
                ..
            } = load_scenario_str(&alt_toml)
                .with_context(|| "failed to load alt scenario for replay")?;
            validate_scenario(&alt_scenario).with_context(|| "alt scenario validation failed")?;
            let live_alt_hash = manifest::scenario_hash(&alt_scenario)
                .with_context(|| "failed to hash alt scenario")?;
            if live_alt_hash != *alt_scenario_hash {
                anyhow::bail!(
                    "alt scenario hash mismatch:\n  saved:    {}\n  live:     {}",
                    alt_scenario_hash,
                    live_alt_hash
                );
            }
            let alt_label = alt_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("alt")
                .to_string();
            let report = faultline_stats::counterfactual::run_compare(
                scenario,
                &alt_scenario,
                &alt_label,
                &config,
            )
            .with_context(|| "compare replay failed")?;
            let h =
                manifest::output_hash(&report).with_context(|| "failed to hash compare replay")?;
            (
                ManifestMode::Compare {
                    alt_scenario_path: alt_scenario_path.clone(),
                    alt_scenario_hash: alt_scenario_hash.clone(),
                },
                h,
            )
        },
        ManifestMode::Search {
            method,
            trials,
            search_seed,
            objectives,
        } => {
            // Reparse the recorded objective labels back into the
            // structured form. Recording the labels (not the structured
            // enum) keeps the manifest stable across future objective
            // additions; reparsing here checks the recorded labels are
            // still recognised by the current build.
            let parsed_objectives: Vec<SearchObjective> = objectives
                .iter()
                .map(|s| SearchObjective::parse_cli(s).map_err(anyhow::Error::msg))
                .collect::<Result<Vec<_>>>()
                .with_context(|| {
                    "failed to reparse search objective labels from manifest \
                     (a recorded label is no longer recognised)"
                })?;
            let inner_config = config.clone();
            let search_config = SearchConfig {
                trials: *trials,
                method: *method,
                search_seed: *search_seed,
                mc_config: inner_config,
                objectives: parsed_objectives,
            };
            let result = faultline_stats::search::run_search(scenario, &search_config)
                .with_context(|| "search replay failed")?;
            let h =
                manifest::output_hash(&result).with_context(|| "failed to hash search replay")?;
            (
                ManifestMode::Search {
                    method: *method,
                    trials: *trials,
                    search_seed: *search_seed,
                    objectives: objectives.clone(),
                },
                h,
            )
        },
        ManifestMode::Sensitivity {
            param,
            low,
            high,
            steps,
            runs_per_step,
        } => {
            // Sensitivity uses its own per-step config: `runs_per_step`
            // overrides the manifest's `num_runs` for the inner MC,
            // but the base seed is shared.
            let inner_config = MonteCarloConfig {
                num_runs: *runs_per_step,
                seed: Some(saved.mc_config.base_seed),
                collect_snapshots: false,
                parallel: false,
            };
            let result = faultline_stats::sensitivity::run_sensitivity(
                scenario,
                &inner_config,
                param,
                *low,
                *high,
                *steps,
            )
            .with_context(|| "sensitivity replay failed")?;
            let h = manifest::output_hash(&result)
                .with_context(|| "failed to hash sensitivity replay")?;
            (
                ManifestMode::Sensitivity {
                    param: param.clone(),
                    low: *low,
                    high: *high,
                    steps: *steps,
                    runs_per_step: *runs_per_step,
                },
                h,
            )
        },
    };

    manifest::build_manifest(
        scenario_path,
        scenario_hash,
        saved.mc_config.clone(),
        mode,
        output_hash,
    )
    .with_context(|| "failed to build replay manifest")
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

/// Escape a string for RFC 4180 CSV: double quotes become `""`.
fn csv_escape(s: &str) -> String {
    s.replace('"', "\"\"")
}

fn write_result_json(cli: &Cli, result: &RunResult) -> Result<()> {
    let path = cli.output.join("single_run.json");
    let json =
        serde_json::to_string_pretty(result).with_context(|| "failed to serialize run result")?;
    fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    info!(path = %path.display(), "wrote single run result");
    Ok(())
}

fn write_outputs(
    cli: &Cli,
    result: &MonteCarloResult,
    scenario: &Scenario,
    manifest_obj: Option<&RunManifest>,
) -> Result<()> {
    match cli.format {
        OutputFormat::Json | OutputFormat::Both => {
            write_json_summary(cli, result)?;
            write_markdown_report(cli, result, scenario, manifest_obj)?;
        },
        OutputFormat::Csv => {},
    }

    match cli.format {
        OutputFormat::Csv | OutputFormat::Both => {
            write_csv_summary(cli, result)?;
            write_event_log(cli, result)?;
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

fn write_markdown_report(
    cli: &Cli,
    result: &MonteCarloResult,
    scenario: &Scenario,
    manifest_obj: Option<&RunManifest>,
) -> Result<()> {
    // Only emit the report if there's something Phase-6 to show.
    if result.summary.campaign_summaries.is_empty() && result.summary.feasibility_matrix.is_empty()
    {
        return Ok(());
    }
    let path = cli.output.join("report.md");
    let body = faultline_stats::report::render_markdown(&result.summary, scenario);
    let md = with_manifest_front_matter(&body, manifest_obj);
    fs::write(&path, md).with_context(|| format!("failed to write {}", path.display()))?;
    info!(path = %path.display(), "wrote Markdown analysis report");
    Ok(())
}

/// Prepend a manifest front-matter block to a rendered markdown body.
///
/// Front-matter is an HTML comment plus a one-line "Run manifest:"
/// note. The HTML comment carries the structured manifest hash for
/// scripts (`grep -oP 'manifest_hash="[a-f0-9]+"'`); the prose line is
/// what an analyst sees when scanning the report. When `manifest_obj`
/// is `None`, returns `body` unchanged.
fn with_manifest_front_matter(body: &str, manifest_obj: Option<&RunManifest>) -> String {
    let Some(m) = manifest_obj else {
        return body.to_string();
    };
    let mode_label = match &m.mode {
        ManifestMode::SingleRun => "single-run".to_string(),
        ManifestMode::MonteCarlo => "monte-carlo".to_string(),
        ManifestMode::Counterfactual { .. } => "counterfactual".to_string(),
        ManifestMode::Compare { .. } => "compare".to_string(),
        ManifestMode::Sensitivity { .. } => "sensitivity".to_string(),
        ManifestMode::Search { .. } => "search".to_string(),
    };
    format!(
        "<!-- faultline-manifest manifest_hash=\"{mh}\" output_hash=\"{oh}\" engine_version=\"{ev}\" mode=\"{ml}\" -->\n\n> **Run manifest:** `{mh_short}` (engine `{ev}`, mode `{ml}`, seed `{seed}`, runs `{runs}`). Replay with `faultline scenario.toml --verify manifest.json`.\n\n{body}",
        mh = m.manifest_hash,
        mh_short = short_hash(&m.manifest_hash),
        oh = m.output_hash,
        ev = m.engine_version,
        ml = mode_label,
        seed = m.mc_config.base_seed,
        runs = m.mc_config.num_runs,
        body = body,
    )
}

/// Truncate a hex hash for display. Keeps the first 12 characters —
/// that's 48 bits, well below collision risk for any plausible
/// scenario library and short enough to read in a sentence.
fn short_hash(h: &str) -> String {
    if h.len() <= 12 {
        h.to_string()
    } else {
        h[..12].to_string()
    }
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
            "{},{},\"{}\",\"{}\",{},{}",
            run.run_index,
            run.seed,
            csv_escape(&victor),
            csv_escape(vc),
            run.final_tick,
            run.outcome.final_tension,
        ));
    }

    fs::write(&path, lines.join("\n") + "\n")
        .with_context(|| format!("failed to write {}", path.display()))?;
    info!(path = %path.display(), "wrote CSV runs");
    Ok(())
}

fn write_event_log(cli: &Cli, result: &MonteCarloResult) -> Result<()> {
    let path = cli.output.join("event_log.csv");

    let mut lines = vec!["run_index,tick,event_id".to_string()];

    for run in &result.runs {
        for record in &run.event_log {
            lines.push(format!(
                "{},{},\"{}\"",
                run.run_index,
                record.tick,
                csv_escape(&record.event_id.to_string())
            ));
        }
    }

    if lines.len() > 1 {
        fs::write(&path, lines.join("\n") + "\n")
            .with_context(|| format!("failed to write {}", path.display()))?;
        info!(path = %path.display(), events = lines.len() - 1, "wrote event log CSV");
    } else {
        info!("no events fired across runs, skipping event_log.csv");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// CLI argument parsing tests
// ---------------------------------------------------------------------------
//
// These tests pin the clap-level conflict declarations on `--search`
// against the other run-mode flags. Clap surfaces an `ArgumentConflict`
// kind at parse time when two mutually-exclusive flags appear together.
// We assert on the kind (not the message text) so future clap upgrades
// that change wording don't break the test.

#[cfg(test)]
mod cli_tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn search_and_verify_are_mutually_exclusive() {
        let res = Cli::try_parse_from([
            "faultline",
            "scenario.toml",
            "--search",
            "--verify",
            "manifest.json",
        ]);
        let err = res.expect_err("--search + --verify must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn search_and_validate_are_mutually_exclusive() {
        let res = Cli::try_parse_from(["faultline", "scenario.toml", "--search", "--validate"]);
        let err = res.expect_err("--search + --validate must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn search_and_migrate_are_mutually_exclusive() {
        let res = Cli::try_parse_from(["faultline", "scenario.toml", "--search", "--migrate"]);
        let err = res.expect_err("--search + --migrate must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn search_and_compare_are_mutually_exclusive() {
        let res = Cli::try_parse_from([
            "faultline",
            "scenario.toml",
            "--search",
            "--compare",
            "other.toml",
        ]);
        let err = res.expect_err("--search + --compare must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn search_and_counterfactual_are_mutually_exclusive() {
        let res = Cli::try_parse_from([
            "faultline",
            "scenario.toml",
            "--search",
            "--counterfactual",
            "x=1",
        ]);
        let err = res.expect_err("--search + --counterfactual must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn search_and_sensitivity_are_mutually_exclusive() {
        let res = Cli::try_parse_from(["faultline", "scenario.toml", "--search", "--sensitivity"]);
        let err = res.expect_err("--search + --sensitivity must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn search_and_single_run_are_mutually_exclusive() {
        let res = Cli::try_parse_from(["faultline", "scenario.toml", "--search", "--single-run"]);
        let err = res.expect_err("--search + --single-run must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn search_subflags_require_search() {
        // `--search-trials 8` without `--search` should be rejected by
        // the `requires = "search"` declaration.
        let res = Cli::try_parse_from(["faultline", "scenario.toml", "--search-trials", "8"]);
        let err = res.expect_err("--search-trials without --search must reject");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn search_method_default_is_random() {
        // Sanity check on the clap default: --search alone resolves to
        // SearchMethod::Random.
        let cli = Cli::try_parse_from(["faultline", "scenario.toml", "--search"])
            .expect("--search alone must parse");
        assert!(matches!(cli.search_method, super::CliSearchMethod::Random));
    }

    #[test]
    fn search_objective_can_be_repeated() {
        let cli = Cli::try_parse_from([
            "faultline",
            "scenario.toml",
            "--search",
            "--search-objective",
            "minimize_detection",
            "--search-objective",
            "minimize_duration",
        ])
        .expect("repeated --search-objective must parse");
        assert_eq!(cli.search_objective.len(), 2);
    }
}
