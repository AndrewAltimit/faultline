//! Headless CLI for batch Monte Carlo simulation with Faultline.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use rayon::prelude::*;
use tracing::{error, info};

use faultline_engine::{Engine, validate_scenario};
use faultline_stats::compute_summary;
use faultline_stats::counterfactual::{ComparisonReport, ParamOverride};
use faultline_types::scenario::Scenario;
use faultline_types::stats::{MonteCarloConfig, MonteCarloResult, RunResult};

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
        return run_sensitivity_analysis(&cli, &scenario);
    }

    if !cli.counterfactual.is_empty() {
        return run_counterfactual_analysis(&cli, &scenario);
    }

    if let Some(ref alt_path) = cli.compare {
        return run_compare_analysis(&cli, &scenario, alt_path);
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

    write_outputs(cli, &mc_result, scenario)?;

    Ok(())
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

    write_comparison_outputs(cli, &report, scenario)?;

    Ok(())
}

fn run_compare_analysis(cli: &Cli, scenario: &Scenario, alt_path: &PathBuf) -> Result<()> {
    let alt_toml = fs::read_to_string(alt_path)
        .with_context(|| format!("failed to read --compare scenario: {}", alt_path.display()))?;
    let alt_scenario: Scenario = toml::from_str(&alt_toml).with_context(|| {
        format!(
            "failed to parse --compare scenario TOML: {}",
            alt_path.display()
        )
    })?;
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

    write_comparison_outputs(cli, &report, scenario)?;

    Ok(())
}

fn write_comparison_outputs(
    cli: &Cli,
    report: &ComparisonReport,
    scenario: &Scenario,
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
        let md = faultline_stats::report::render_comparison_markdown(report, scenario);
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

fn write_outputs(cli: &Cli, result: &MonteCarloResult, scenario: &Scenario) -> Result<()> {
    match cli.format {
        OutputFormat::Json | OutputFormat::Both => {
            write_json_summary(cli, result)?;
            write_markdown_report(cli, result, scenario)?;
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

fn write_markdown_report(cli: &Cli, result: &MonteCarloResult, scenario: &Scenario) -> Result<()> {
    // Only emit the report if there's something Phase-6 to show.
    if result.summary.campaign_summaries.is_empty() && result.summary.feasibility_matrix.is_empty()
    {
        return Ok(());
    }
    let path = cli.output.join("report.md");
    let md = faultline_stats::report::render_markdown(&result.summary, scenario);
    fs::write(&path, md).with_context(|| format!("failed to write {}", path.display()))?;
    info!(path = %path.display(), "wrote Markdown analysis report");
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
