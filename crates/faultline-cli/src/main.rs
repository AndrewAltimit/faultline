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

    /// Run strategy search.
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
            "compare", "verify", "validate", "migrate", "robustness"
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

    /// Run adversarial co-evolution.
    ///
    /// Alternates best-response moves between an attacker faction and
    /// a defender faction over the scenario's `[strategy_space]` until
    /// both sides' assignments stabilize (Nash equilibrium in pure
    /// strategies on the discrete strategy space the search visits),
    /// a cycle of any period is detected, or `--coevolve-rounds` is
    /// reached.
    ///
    /// Every `[strategy_space.variables]` entry must declare an `owner`
    /// matching either `--coevolve-attacker` or `--coevolve-defender`;
    /// the runner partitions variables by owner so each side
    /// reoptimizes only the parameters it controls.
    ///
    /// Mutually exclusive with the other run modes.
    #[arg(
        long = "coevolve",
        conflicts_with_all = [
            "single_run", "sensitivity", "counterfactual",
            "compare", "search", "verify", "validate", "migrate", "robustness"
        ]
    )]
    coevolve: bool,

    /// Maximum rounds for `--coevolve`. One round = one side's best
    /// response. Defaults to 8.
    #[arg(long = "coevolve-rounds", default_value_t = 8, requires = "coevolve")]
    coevolve_rounds: u32,

    /// Attacker faction ID for `--coevolve`. Must match an existing
    /// faction; the strategy_space variables tagged with this faction
    /// as `owner` are the attacker's decision variables.
    #[arg(
        long = "coevolve-attacker",
        value_name = "FACTION_ID",
        requires = "coevolve"
    )]
    coevolve_attacker: Option<String>,

    /// Defender faction ID for `--coevolve`. See `--coevolve-attacker`.
    #[arg(
        long = "coevolve-defender",
        value_name = "FACTION_ID",
        requires = "coevolve"
    )]
    coevolve_defender: Option<String>,

    /// Attacker objective for `--coevolve`. Same format as
    /// `--search-objective`. Required.
    #[arg(
        long = "coevolve-attacker-objective",
        value_name = "OBJECTIVE",
        requires = "coevolve"
    )]
    coevolve_attacker_objective: Option<String>,

    /// Defender objective for `--coevolve`. Same format as
    /// `--search-objective`. Required.
    #[arg(
        long = "coevolve-defender-objective",
        value_name = "OBJECTIVE",
        requires = "coevolve"
    )]
    coevolve_defender_objective: Option<String>,

    /// Sampling method for both sides' per-round sub-search. Defaults
    /// to `grid` so small spaces produce reproducible per-round
    /// best responses.
    #[arg(
        long = "coevolve-method",
        value_name = "METHOD",
        default_value = "grid",
        requires = "coevolve"
    )]
    coevolve_method: CliSearchMethod,

    /// Trials per round per side for `--coevolve`. Each round runs an
    /// independent inner Monte Carlo batch sized by `--coevolve-runs`
    /// for each trial.
    #[arg(long = "coevolve-trials", default_value_t = 8, requires = "coevolve")]
    coevolve_trials: u32,

    /// Inner Monte Carlo run count per trial in `--coevolve`.
    #[arg(long = "coevolve-runs", default_value_t = 30, requires = "coevolve")]
    coevolve_runs: u32,

    /// Co-evolution-only RNG seed. Drives the per-round sub-search
    /// sampler via `coevolve_seed.wrapping_add(round_index)`.
    /// Independent of `--seed` (the inner Monte Carlo seed).
    #[arg(long = "coevolve-seed", requires = "coevolve")]
    coevolve_seed: Option<u64>,

    /// Side to move first in `--coevolve`. Defaults to `defender` —
    /// the most-common analyst question is "given my fixed posture,
    /// how does the attacker adapt?", so we let the defender commit
    /// first.
    #[arg(
        long = "coevolve-initial-mover",
        value_name = "SIDE",
        default_value = "defender",
        requires = "coevolve"
    )]
    coevolve_initial_mover: CliCoevolveSide,

    /// Run defender-posture robustness analysis.
    ///
    /// Evaluates each defender posture against every attacker profile
    /// declared in `[strategy_space.attacker_profiles]` and surfaces
    /// per-posture worst/best/mean across profiles. The expected
    /// workflow is `--search` followed by `--robustness
    /// --robustness-from-search ./output/search.json`: first identify
    /// Pareto-optimal defender postures against a single (implicit)
    /// attacker baseline, then re-rank them by worst-case profile.
    ///
    /// When `--robustness-from-search` is omitted, the runner evaluates
    /// every profile against the scenario's natural state — useful for
    /// sanity-checking that profiles apply cleanly before running a
    /// full search.
    ///
    /// Mutually exclusive with the other run modes.
    #[arg(
        long = "robustness",
        conflicts_with_all = [
            "single_run", "sensitivity", "counterfactual",
            "compare", "search", "coevolve",
            "verify", "validate", "migrate"
        ]
    )]
    robustness: bool,

    /// Path to a saved `search.json` whose Pareto-frontier trials
    /// supply the defender postures evaluated by `--robustness`. When
    /// omitted, only the scenario's natural state is evaluated.
    ///
    /// The CLI re-hashes the file at run time and records the hash in
    /// the manifest so `--verify` refuses a stale source file. This
    /// mirrors how `--compare` handles the alt scenario file.
    #[arg(
        long = "robustness-from-search",
        value_name = "SEARCH_JSON",
        requires = "robustness"
    )]
    robustness_from_search: Option<PathBuf>,
    /// Inner Monte Carlo run count for each (posture, profile) cell.
    /// Defaults to 100 — same default as `--search-runs`. Smaller
    /// values trade noisier per-cell estimates for faster turnaround.
    #[arg(
        long = "robustness-runs",
        default_value_t = 100,
        requires = "robustness"
    )]
    robustness_runs: u32,

    /// Robustness objective. Pass repeatedly to evaluate the cells
    /// against multiple metrics. Same format as `--search-objective`.
    /// When omitted, falls back to the scenario's
    /// `[strategy_space].objectives`; if that is also empty the run
    /// fails with a clear error.
    #[arg(
        long = "robustness-objective",
        value_name = "OBJECTIVE",
        requires = "robustness"
    )]
    robustness_objective: Vec<String>,

    /// Skip the natural-state baseline row in `--robustness`. By
    /// default the runner prepends a `posture = baseline` row so
    /// per-posture deltas read against "do nothing" rather than
    /// against an arbitrary trial.
    #[arg(
        long = "robustness-skip-baseline",
        default_value_t = false,
        requires = "robustness"
    )]
    robustness_skip_baseline: bool,

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

    /// Emit a structured "what does this scenario actually model?"
    /// summary.
    ///
    /// Surfaces factions, kill chains, victory conditions, the
    /// `[strategy_space]` decision-variable surface, and any
    /// author-flagged Low-confidence parameters. Pure schema operation
    /// — no engine invocation, no RNG. Prints to stdout (Markdown by
    /// default; pass `--explain-format json` for the structured form
    /// suitable for tooling). Redirect to a file via shell if you want
    /// a saved artifact.
    ///
    /// Mutually exclusive with the run modes (`--single-run`,
    /// `--sensitivity`, `--counterfactual`, `--compare`, `--search`,
    /// `--coevolve`, `--robustness`, `--verify`, `--migrate`).
    #[arg(
        long = "explain",
        conflicts_with_all = [
            "single_run", "sensitivity", "counterfactual",
            "compare", "search", "coevolve", "robustness",
            "verify", "migrate"
        ]
    )]
    explain: bool,

    /// Output format for `--explain`. `markdown` (default) emits
    /// human-readable Markdown; `json` emits the structured
    /// [`ExplainReport`] for tooling.
    #[arg(
        long = "explain-format",
        value_name = "FORMAT",
        default_value = "markdown",
        requires = "explain"
    )]
    explain_format: ExplainFormat,
}

/// Output format for `--explain`.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum ExplainFormat {
    Markdown,
    Json,
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

/// CLI form of `faultline_stats::coevolve::CoevolveSide` so clap can
/// parse `--coevolve-initial-mover defender` into the structured enum.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum CliCoevolveSide {
    Attacker,
    Defender,
}

impl From<CliCoevolveSide> for faultline_stats::coevolve::CoevolveSide {
    fn from(s: CliCoevolveSide) -> Self {
        match s {
            CliCoevolveSide::Attacker => faultline_stats::coevolve::CoevolveSide::Attacker,
            CliCoevolveSide::Defender => faultline_stats::coevolve::CoevolveSide::Defender,
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

    if cli.explain {
        return run_explain(&cli, &scenario);
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

    if cli.coevolve {
        return run_coevolve_analysis(&cli, &scenario);
    }

    if cli.robustness {
        return run_robustness_analysis(&cli, &scenario);
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
// Counterfactual & comparison
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
// Strategy search
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
        // Always compute the baseline for CLI search runs — the
        // Counter-Recommendation report section needs it as
        // the comparison anchor. The cost is one extra MC batch per
        // search, which is negligible compared to the trial budget.
        compute_baseline: true,
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
        compute_baseline: config.compute_baseline,
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
// Co-evolution
// ---------------------------------------------------------------------------

fn run_coevolve_analysis(cli: &Cli, scenario: &Scenario) -> Result<()> {
    use faultline_stats::coevolve::{CoevolveConfig, CoevolveSideConfig, run_coevolution};

    // All four required strings must be present before we touch the
    // engine — refuse early with a single error rather than letting
    // each Option::expect cascade.
    let attacker_id = cli
        .coevolve_attacker
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--coevolve-attacker is required for --coevolve"))?;
    let defender_id = cli
        .coevolve_defender
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--coevolve-defender is required for --coevolve"))?;
    let attacker_obj_str = cli
        .coevolve_attacker_objective
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--coevolve-attacker-objective is required"))?;
    let defender_obj_str = cli
        .coevolve_defender_objective
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--coevolve-defender-objective is required"))?;

    let attacker_objective = SearchObjective::parse_cli(attacker_obj_str)
        .map_err(anyhow::Error::msg)
        .with_context(|| "failed to parse --coevolve-attacker-objective")?;
    let defender_objective = SearchObjective::parse_cli(defender_obj_str)
        .map_err(anyhow::Error::msg)
        .with_context(|| "failed to parse --coevolve-defender-objective")?;

    let mc_seed = cli
        .seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));
    let coevolve_seed = cli
        .coevolve_seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));

    let mc_config = MonteCarloConfig {
        num_runs: cli.coevolve_runs,
        seed: Some(mc_seed),
        collect_snapshots: false,
        parallel: false,
    };

    let method: SearchMethod = cli.coevolve_method.into();
    let initial_mover = cli.coevolve_initial_mover.into();

    let attacker_faction = faultline_types::ids::FactionId::from(attacker_id);
    let defender_faction = faultline_types::ids::FactionId::from(defender_id);
    let assignment_tolerance = 1e-9;

    let config = CoevolveConfig {
        max_rounds: cli.coevolve_rounds,
        initial_mover,
        attacker: CoevolveSideConfig {
            faction: attacker_faction.clone(),
            objective: attacker_objective.clone(),
            method,
            trials: cli.coevolve_trials,
        },
        defender: CoevolveSideConfig {
            faction: defender_faction.clone(),
            objective: defender_objective.clone(),
            method,
            trials: cli.coevolve_trials,
        },
        mc_config,
        coevolve_seed,
        assignment_tolerance,
    };

    info!(
        max_rounds = cli.coevolve_rounds,
        attacker = %attacker_faction,
        defender = %defender_faction,
        method = ?method,
        trials = cli.coevolve_trials,
        runs = cli.coevolve_runs,
        coevolve_seed,
        mc_seed,
        "starting co-evolution"
    );

    let result = run_coevolution(scenario, &config).with_context(|| "co-evolution run failed")?;

    write_coevolve_outputs(cli, &result)?;

    let manifest_mc = ManifestMcConfig::from_config(&config.mc_config, mc_seed);
    let mode = ManifestMode::Coevolve {
        max_rounds: cli.coevolve_rounds,
        coevolve_seed,
        initial_mover,
        attacker_faction,
        defender_faction,
        attacker_objective: attacker_objective.label(),
        defender_objective: defender_objective.label(),
        attacker_method: method,
        defender_method: method,
        attacker_trials: cli.coevolve_trials,
        defender_trials: cli.coevolve_trials,
        assignment_tolerance,
    };
    let output_hash =
        manifest::output_hash(&result).with_context(|| "failed to hash co-evolve result")?;
    let manifest_obj = build_manifest_object(cli, scenario, manifest_mc, mode, output_hash)?;

    if matches!(cli.format, OutputFormat::Csv) {
        tracing::warn!(
            "--format csv is not meaningful for co-evolve output \
             (per-round CSV shape doesn't apply); falling back to JSON + Markdown"
        );
    }
    let md_path = cli.output.join("coevolve_report.md");
    let body = faultline_stats::report::render_coevolve_markdown(&result, scenario);
    let md = with_manifest_front_matter(&body, Some(&manifest_obj));
    fs::write(&md_path, md).with_context(|| format!("failed to write {}", md_path.display()))?;
    info!(path = %md_path.display(), "wrote co-evolve Markdown report");

    write_manifest_object(cli, &manifest_obj)?;

    // Emit a one-line summary on stdout so a script driving the CLI
    // (or a CI step) can grep for the convergence outcome without
    // parsing the JSON. Matches the `VERIFY OK` pattern used by
    // `--verify`.
    let status_label = match &result.status {
        faultline_stats::coevolve::CoevolveStatus::Converged => "converged".to_string(),
        faultline_stats::coevolve::CoevolveStatus::Cycle { period } => {
            format!("cycle:{period}")
        },
        faultline_stats::coevolve::CoevolveStatus::NoEquilibrium => "no_equilibrium".to_string(),
    };
    println!(
        "COEVOLVE {status} rounds={rounds} manifest_hash={mh}",
        status = status_label,
        rounds = result.rounds.len(),
        mh = manifest_obj.manifest_hash,
    );
    Ok(())
}

fn write_coevolve_outputs(
    cli: &Cli,
    result: &faultline_stats::coevolve::CoevolveResult,
) -> Result<()> {
    let json_path = cli.output.join("coevolve.json");
    let json = serde_json::to_string_pretty(result)
        .with_context(|| "failed to serialize co-evolve result")?;
    fs::write(&json_path, json)
        .with_context(|| format!("failed to write {}", json_path.display()))?;
    info!(path = %json_path.display(), "wrote co-evolve JSON");
    Ok(())
}

// ---------------------------------------------------------------------------
// Robustness
// ---------------------------------------------------------------------------

fn run_robustness_analysis(cli: &Cli, scenario: &Scenario) -> Result<()> {
    use faultline_stats::manifest::{ManifestAssignment, ManifestPosture};
    use faultline_stats::robustness::{RobustnessConfig, run_robustness};

    // Resolve objectives. Same pattern as `--search`: CLI flags
    // override the embedded scenario list, and an empty intersection
    // is rejected by the runner with a clear error.
    let objectives: Vec<SearchObjective> = if cli.robustness_objective.is_empty() {
        scenario.strategy_space.objectives.clone()
    } else {
        cli.robustness_objective
            .iter()
            .map(|s| SearchObjective::parse_cli(s).map_err(anyhow::Error::msg))
            .collect::<Result<Vec<_>>>()
            .with_context(|| "failed to parse --robustness-objective")?
    };

    // Resolve postures. Either lifted from a saved search.json or
    // empty (in which case the runner just evaluates the natural-state
    // baseline). The hash of any source file is captured for manifest
    // verification.
    let (postures, from_search_path, from_search_hash) =
        load_robustness_postures(cli).with_context(|| "failed to load robustness postures")?;

    let mc_seed = cli
        .seed
        .unwrap_or_else(|| rand::Rng::r#gen::<u64>(&mut rand::thread_rng()));
    let mc_config = MonteCarloConfig {
        num_runs: cli.robustness_runs,
        seed: Some(mc_seed),
        collect_snapshots: false,
        parallel: false,
    };

    let include_baseline = !cli.robustness_skip_baseline;
    let config = RobustnessConfig {
        postures: postures.clone(),
        include_baseline,
        mc_config: mc_config.clone(),
        objectives: objectives.clone(),
    };

    info!(
        postures = postures.len(),
        include_baseline,
        runs = cli.robustness_runs,
        objectives = objectives.len(),
        "starting robustness analysis"
    );

    let result = run_robustness(scenario, &config).with_context(|| "robustness analysis failed")?;

    write_robustness_outputs(cli, &result)?;

    let manifest_postures: Vec<ManifestPosture> = postures
        .iter()
        .map(|p| ManifestPosture {
            label: p.label.clone(),
            assignments: p
                .assignments
                .iter()
                .map(|a| ManifestAssignment {
                    path: a.path.clone(),
                    value: a.value,
                })
                .collect(),
        })
        .collect();
    let manifest_mc = ManifestMcConfig::from_config(&mc_config, mc_seed);
    let mode = ManifestMode::Robustness {
        objectives: objectives.iter().map(SearchObjective::label).collect(),
        include_baseline,
        postures: manifest_postures,
        from_search_path,
        from_search_hash,
    };
    let output_hash =
        manifest::output_hash(&result).with_context(|| "failed to hash robustness result")?;
    let manifest_obj = build_manifest_object(cli, scenario, manifest_mc, mode, output_hash)?;

    if matches!(cli.format, OutputFormat::Csv) {
        tracing::warn!(
            "--format csv is not meaningful for robustness output \
             (per-cell CSV shape doesn't apply); falling back to JSON + Markdown"
        );
    }
    let md_path = cli.output.join("robustness_report.md");
    let body = faultline_stats::report::render_robustness_markdown(&result, scenario);
    let md = with_manifest_front_matter(&body, Some(&manifest_obj));
    fs::write(&md_path, md).with_context(|| format!("failed to write {}", md_path.display()))?;
    info!(path = %md_path.display(), "wrote robustness Markdown report");

    write_manifest_object(cli, &manifest_obj)?;
    Ok(())
}

fn write_robustness_outputs(
    cli: &Cli,
    result: &faultline_stats::robustness::RobustnessResult,
) -> Result<()> {
    let json_path = cli.output.join("robustness.json");
    let json = serde_json::to_string_pretty(result)
        .with_context(|| "failed to serialize robustness result")?;
    fs::write(&json_path, json)
        .with_context(|| format!("failed to write {}", json_path.display()))?;
    info!(path = %json_path.display(), "wrote robustness JSON");
    Ok(())
}

/// Load defender postures for a robustness run, from either an inline
/// list or a saved `search.json`. Returns the postures, the source
/// file path (relative to CWD) and its SHA-256 — both `None` when no
/// source file was supplied.
fn load_robustness_postures(
    cli: &Cli,
) -> Result<(
    Vec<faultline_stats::robustness::DefenderPosture>,
    Option<String>,
    Option<String>,
)> {
    use faultline_stats::robustness::DefenderPosture;
    let Some(ref path) = cli.robustness_from_search else {
        return Ok((Vec::new(), None, None));
    };

    // Same path-safety check `--compare` applies to its alt scenario:
    // refuse absolute paths and parent traversals so a crafted command
    // line can't read arbitrary files via this flag.
    if path.is_absolute() {
        anyhow::bail!(
            "--robustness-from-search path is absolute, refusing for safety: {}",
            path.display()
        );
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        anyhow::bail!(
            "--robustness-from-search path contains parent traversal, refusing for safety: {}",
            path.display()
        );
    }

    let bytes = fs::read(path).with_context(|| {
        format!(
            "failed to read --robustness-from-search: {}",
            path.display()
        )
    })?;
    let hash = manifest::sha256_hex(&bytes);
    let saved: SearchResult = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "failed to parse --robustness-from-search as a SearchResult JSON: {}",
            path.display()
        )
    })?;

    // Resolve every Pareto index explicitly. A `filter_map` here would
    // silently drop stale or out-of-bounds indices (a hand-edited or
    // corrupted `search.json` could quietly shrink the posture set
    // without the analyst noticing); fail loudly instead so the
    // mismatch is named.
    let mut postures: Vec<DefenderPosture> = Vec::with_capacity(saved.pareto_indices.len());
    for idx in &saved.pareto_indices {
        let trial = saved.trials.get(*idx as usize).ok_or_else(|| {
            anyhow::anyhow!(
                "--robustness-from-search references Pareto index {idx} but the saved \
                 SearchResult only has {n} trial(s); the source file is stale or corrupted",
                idx = idx,
                n = saved.trials.len(),
            )
        })?;
        postures.push(DefenderPosture {
            label: format!("posture_{}", idx),
            assignments: trial.assignments.clone(),
        });
    }
    // An empty Pareto frontier (degenerate search artifact) is a valid
    // input: the caller may still want to evaluate the natural-state
    // baseline against any declared profiles via `--robustness-skip-baseline=false`.
    // Defer the empty-vs-no-baseline rejection to `run_robustness`, which
    // already enforces "at least one posture or include_baseline=true".
    Ok((postures, Some(path.display().to_string()), Some(hash)))
}

// ---------------------------------------------------------------------------
// Schema migration
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
// ---------------------------------------------------------------------------
// Explain
// ---------------------------------------------------------------------------
//
// Pure schema view: build the explain report and print it to stdout.
// Mirrors `--migrate`'s stdout-by-default contract — `print!` (not
// `info!`) so a redirect (`> explain.md`) captures only the report
// bytes. Selecting `--explain-format json` swaps the renderer; both
// shapes share the same producer.
fn run_explain(cli: &Cli, scenario: &Scenario) -> Result<()> {
    let report = faultline_stats::explain::explain(scenario);
    match cli.explain_format {
        ExplainFormat::Markdown => {
            let md = faultline_stats::explain::render_markdown(&report);
            print!("{md}");
        },
        ExplainFormat::Json => {
            let json = serde_json::to_string_pretty(&report)
                .with_context(|| "failed to serialize explain report as JSON")?;
            println!("{json}");
        },
    }
    Ok(())
}

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
// Manifest emission + verify
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
            compute_baseline,
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
                compute_baseline: *compute_baseline,
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
                    compute_baseline: *compute_baseline,
                },
                h,
            )
        },
        ManifestMode::Coevolve {
            max_rounds,
            coevolve_seed,
            initial_mover,
            attacker_faction,
            defender_faction,
            attacker_objective,
            defender_objective,
            attacker_method,
            defender_method,
            attacker_trials,
            defender_trials,
            assignment_tolerance,
        } => {
            use faultline_stats::coevolve::{CoevolveConfig, CoevolveSideConfig, run_coevolution};

            let attacker_obj = SearchObjective::parse_cli(attacker_objective)
                .map_err(anyhow::Error::msg)
                .with_context(|| {
                    "failed to reparse coevolve attacker objective from manifest \
                     (a recorded label is no longer recognised)"
                })?;
            let defender_obj = SearchObjective::parse_cli(defender_objective)
                .map_err(anyhow::Error::msg)
                .with_context(|| {
                    "failed to reparse coevolve defender objective from manifest \
                     (a recorded label is no longer recognised)"
                })?;
            let coevolve_cfg = CoevolveConfig {
                max_rounds: *max_rounds,
                initial_mover: *initial_mover,
                attacker: CoevolveSideConfig {
                    faction: attacker_faction.clone(),
                    objective: attacker_obj,
                    method: *attacker_method,
                    trials: *attacker_trials,
                },
                defender: CoevolveSideConfig {
                    faction: defender_faction.clone(),
                    objective: defender_obj,
                    method: *defender_method,
                    trials: *defender_trials,
                },
                mc_config: config.clone(),
                coevolve_seed: *coevolve_seed,
                assignment_tolerance: *assignment_tolerance,
            };
            let result = run_coevolution(scenario, &coevolve_cfg)
                .with_context(|| "co-evolve replay failed")?;
            let h = manifest::output_hash(&result)
                .with_context(|| "failed to hash co-evolve replay")?;
            (
                ManifestMode::Coevolve {
                    max_rounds: *max_rounds,
                    coevolve_seed: *coevolve_seed,
                    initial_mover: *initial_mover,
                    attacker_faction: attacker_faction.clone(),
                    defender_faction: defender_faction.clone(),
                    attacker_objective: attacker_objective.clone(),
                    defender_objective: defender_objective.clone(),
                    attacker_method: *attacker_method,
                    defender_method: *defender_method,
                    attacker_trials: *attacker_trials,
                    defender_trials: *defender_trials,
                    assignment_tolerance: *assignment_tolerance,
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
        ManifestMode::Robustness {
            objectives,
            include_baseline,
            postures,
            from_search_path,
            from_search_hash,
        } => {
            use faultline_stats::counterfactual::ParamOverride;
            use faultline_stats::robustness::{DefenderPosture, RobustnessConfig, run_robustness};

            // If a source search file was recorded, re-read it and
            // refuse on hash mismatch — same safety pattern as
            // ManifestMode::Compare's alt scenario check. We don't
            // re-derive postures from the file (the manifest already
            // carries them frozen); we only verify the source hasn't
            // drifted, so an analyst chasing reproducibility against
            // the original search artifact has a one-step check.
            //
            // Why: a hand-crafted manifest with only one of the two
            // fields populated would otherwise silently skip the
            // hash check (the if-let-tuple needs both Some to fire).
            // The CLI always writes both together, so a half-populated
            // pair is a malformed manifest, not a soft fallback.
            if from_search_path.is_some() != from_search_hash.is_some() {
                anyhow::bail!(
                    "manifest is malformed: robustness from_search_path and \
                     from_search_hash must both be present or both absent \
                     (got path={}, hash={})",
                    if from_search_path.is_some() {
                        "Some"
                    } else {
                        "None"
                    },
                    if from_search_hash.is_some() {
                        "Some"
                    } else {
                        "None"
                    },
                );
            }
            if let (Some(rel), Some(saved_hash)) = (from_search_path, from_search_hash) {
                let p = Path::new(rel);
                if p.is_absolute() {
                    anyhow::bail!(
                        "robustness from_search_path in manifest is absolute, refusing: {}",
                        rel
                    );
                }
                if p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    anyhow::bail!(
                        "robustness from_search_path in manifest contains parent traversal, refusing: {}",
                        rel
                    );
                }
                let live_bytes = fs::read(p).with_context(|| {
                    format!("failed to read robustness source search.json: {}", rel)
                })?;
                let live_hash = manifest::sha256_hex(&live_bytes);
                if &live_hash != saved_hash {
                    anyhow::bail!(
                        "robustness source search.json hash mismatch:\n  saved:    {}\n  live:     {}",
                        saved_hash,
                        live_hash
                    );
                }
            }

            let parsed_objectives: Vec<SearchObjective> = objectives
                .iter()
                .map(|s| SearchObjective::parse_cli(s).map_err(anyhow::Error::msg))
                .collect::<Result<Vec<_>>>()
                .with_context(|| {
                    "failed to reparse robustness objective labels from manifest \
                     (a recorded label is no longer recognised)"
                })?;
            let lifted_postures: Vec<DefenderPosture> = postures
                .iter()
                .map(|p| DefenderPosture {
                    label: p.label.clone(),
                    assignments: p
                        .assignments
                        .iter()
                        .map(|a| ParamOverride {
                            path: a.path.clone(),
                            value: a.value,
                        })
                        .collect(),
                })
                .collect();
            let robustness_cfg = RobustnessConfig {
                postures: lifted_postures,
                include_baseline: *include_baseline,
                mc_config: config.clone(),
                objectives: parsed_objectives,
            };
            let result = run_robustness(scenario, &robustness_cfg)
                .with_context(|| "robustness replay failed")?;
            let h = manifest::output_hash(&result)
                .with_context(|| "failed to hash robustness replay")?;
            (
                ManifestMode::Robustness {
                    objectives: objectives.clone(),
                    include_baseline: *include_baseline,
                    postures: postures.clone(),
                    from_search_path: from_search_path.clone(),
                    from_search_hash: from_search_hash.clone(),
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
    // Only emit the report if there's something analytical to show:
    // a Phase-6 kill-chain table, Phase-6 feasibility matrix, a
    // network rollup, or an alliance-fracture rollup. Otherwise
    // the report is just the win-rate header and that's already
    // in summary.json.
    if result.summary.campaign_summaries.is_empty()
        && result.summary.feasibility_matrix.is_empty()
        && result.summary.network_summaries.is_empty()
        && result.summary.alliance_dynamics.is_none()
        && result.summary.civilian_activation_summaries.is_empty()
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
        ManifestMode::Coevolve { .. } => "coevolve".to_string(),
        ManifestMode::Robustness { .. } => "robustness".to_string(),
    };
    // Modes whose deterministic replay depends on a second RNG seed
    // beyond `mc_config.base_seed` surface that seed in the prose so an
    // analyst reading only the `.md` report has the full seed pair
    // they need for bit-identical replay (the manifest JSON carries it
    // either way; this is a human-facing convenience).
    let seed_extra = match &m.mode {
        ManifestMode::Coevolve { coevolve_seed, .. } => {
            format!(", coevolve_seed `{coevolve_seed}`")
        },
        _ => String::new(),
    };
    format!(
        "<!-- faultline-manifest manifest_hash=\"{mh}\" output_hash=\"{oh}\" engine_version=\"{ev}\" mode=\"{ml}\" -->\n\n> **Run manifest:** `{mh_short}` (engine `{ev}`, mode `{ml}`, seed `{seed}`{seed_extra}, runs `{runs}`). Replay with `faultline scenario.toml --verify manifest.json`.\n\n{body}",
        mh = m.manifest_hash,
        mh_short = short_hash(&m.manifest_hash),
        oh = m.output_hash,
        ev = m.engine_version,
        ml = mode_label,
        seed = m.mc_config.base_seed,
        seed_extra = seed_extra,
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

    // -- --coevolve flag tests -----------------

    #[test]
    fn coevolve_and_search_are_mutually_exclusive() {
        let res = Cli::try_parse_from(["faultline", "scenario.toml", "--coevolve", "--search"]);
        let err = res.expect_err("--coevolve + --search must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn coevolve_and_verify_are_mutually_exclusive() {
        let res = Cli::try_parse_from([
            "faultline",
            "scenario.toml",
            "--coevolve",
            "--verify",
            "manifest.json",
        ]);
        let err = res.expect_err("--coevolve + --verify must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn coevolve_and_single_run_are_mutually_exclusive() {
        let res = Cli::try_parse_from(["faultline", "scenario.toml", "--coevolve", "--single-run"]);
        let err = res.expect_err("--coevolve + --single-run must conflict");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn coevolve_subflags_require_coevolve() {
        // Each `--coevolve-*` flag declares `requires = "coevolve"` so
        // a stray `--coevolve-rounds 4` without `--coevolve` is rejected
        // at parse time rather than silently ignored under another mode.
        let res = Cli::try_parse_from(["faultline", "scenario.toml", "--coevolve-rounds", "4"]);
        let err = res.expect_err("--coevolve-rounds without --coevolve must reject");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn coevolve_initial_mover_parses_both_sides() {
        for side in ["attacker", "defender"] {
            let cli = Cli::try_parse_from([
                "faultline",
                "scenario.toml",
                "--coevolve",
                "--coevolve-initial-mover",
                side,
            ])
            .expect("initial-mover flag must accept attacker/defender");
            // Force the enum to round-trip through the From impl so a
            // future variant rename here would surface in this test.
            let _: faultline_stats::coevolve::CoevolveSide = cli.coevolve_initial_mover.into();
        }
    }

    #[test]
    fn coevolve_initial_mover_defaults_to_defender() {
        // The CLI default — picked because the most-common analyst
        // question is "given my fixed posture, how does the attacker
        // adapt?" so the defender commits first.
        let cli = Cli::try_parse_from(["faultline", "scenario.toml", "--coevolve"])
            .expect("parse with default initial-mover");
        let resolved: faultline_stats::coevolve::CoevolveSide = cli.coevolve_initial_mover.into();
        assert_eq!(resolved, faultline_stats::coevolve::CoevolveSide::Defender);
    }

    #[test]
    fn coevolve_method_default_is_grid() {
        // Grid is the CLI default because the bundled demo's small
        // spaces enumerate exhaustively, making the per-round best
        // response deterministic without trial-budget tuning.
        let cli = Cli::try_parse_from(["faultline", "scenario.toml", "--coevolve"])
            .expect("parse with default method");
        let resolved: faultline_stats::search::SearchMethod = cli.coevolve_method.into();
        assert_eq!(resolved, faultline_stats::search::SearchMethod::Grid);
    }

    #[test]
    fn explain_is_mutually_exclusive_with_run_modes() {
        // `--explain` is a pure schema view; mixing it with a run mode
        // is almost certainly an authoring mistake. Exercise every
        // entry from the `conflicts_with_all` list so a future
        // refactor that drops one is caught immediately. Value-taking
        // flags (--counterfactual, --compare, --verify) need a dummy
        // value attached or clap rejects them on argument parsing
        // rather than the conflict check we want to assert.
        let cases: &[&[&str]] = &[
            &["--single-run"],
            &["--sensitivity"],
            &["--search"],
            &["--coevolve"],
            &["--robustness"],
            &["--migrate"],
            &["--counterfactual", "faction.alpha.initial_morale=0.3"],
            &["--compare", "other.toml"],
            &["--verify", "manifest.json"],
        ];
        for extra in cases {
            let mut args = vec!["faultline", "scenario.toml", "--explain"];
            args.extend_from_slice(extra);
            let label = extra.join(" ");
            let res = Cli::try_parse_from(&args);
            let err = res.expect_err(&format!(
                "--explain + {label} must conflict but parsed successfully"
            ));
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ArgumentConflict,
                "wrong error kind for --explain + {label}: {err}"
            );
        }
    }

    #[test]
    fn explain_format_requires_explain() {
        // `--explain-format json` without `--explain` would be
        // ambiguous. Reject up-front.
        let res = Cli::try_parse_from(["faultline", "scenario.toml", "--explain-format", "json"]);
        let err = res.expect_err("--explain-format without --explain must reject");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }
}
