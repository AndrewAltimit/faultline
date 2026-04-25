# Faultline

A what-if engine for conflict simulation. Users supply scenario configurations — factions, force compositions, institutional loyalties, political climates, technology modifiers, geographic constraints — and the engine simulates the logical consequences across many runs, producing probability distributions of outcomes.

Faultline is **not** a predictive model. It is a tool for exploring the consequence space of user-defined assumptions.

**Primary interface:** WASM app hosted on GitHub Pages (interactive scenario editor + visualization).
**Secondary interface:** Headless CLI for batch Monte Carlo evaluation (JSON/CSV output).

---

## Quick Start

**Prerequisites:** Rust toolchain (edition 2024) or Docker (v20.10+)

```bash
git clone https://github.com/AndrewAltimit/faultline
cd faultline

# Run directly (requires local Rust toolchain)
cargo test
cargo build --release

# Run a single simulation
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml --single-run

# Run 1000 Monte Carlo simulations
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000

# Counterfactual: re-run with a parameter overridden, get a delta report
cargo run -p faultline-cli -- scenarios/persistent_covert_surveillance.toml -n 1000 \
    --counterfactual "kill_chain.covsurv.phase.covsurv_dwell.detection_probability_per_tick=0.05"

# Side-by-side: run two scenarios with the same seed and report deltas
cargo run -p faultline-cli -- scenarios/tutorial_symmetric.toml -n 1000 \
    --compare scenarios/tutorial_asymmetric.toml

# Or run CI pipeline in Docker (matches GitHub Actions exactly)
docker compose --profile ci run --rm rust-ci cargo fmt --all -- --check
docker compose --profile ci run --rm rust-ci cargo clippy --all-targets -- -D warnings
docker compose --profile ci run --rm rust-ci cargo test
docker compose --profile ci run --rm rust-ci cargo build --release
docker compose --profile ci run --rm rust-ci cargo deny check
```

CI pipeline order: **fmt check -> clippy -> test -> build -> cargo-deny**

## Crate Architecture

```
faultline-types          (shared data structures — no logic, no deps)
    |
    +-- faultline-geo     (geography, maps, terrain)
    +-- faultline-tech    (technology capability cards)
    +-- faultline-politics(political climate, loyalty, defection)
    +-- faultline-events  (event triggers, effects, chains)
    |
    +-- faultline-engine  (core simulation loop — depends on all above)
            |
            +-- faultline-stats         (Monte Carlo runner, statistics)
            +-- faultline-backend-wasm  (browser WASM frontend)
            +-- faultline-cli           (headless batch runner)
```

| Crate | Purpose |
|-------|---------|
| `faultline-types` | Shared data structures (Scenario, Faction, TechCard, etc.) |
| `faultline-geo` | Map loading, terrain modifiers, adjacency graph |
| `faultline-tech` | Technology card effect resolution with terrain modifiers |
| `faultline-politics` | Political climate, institutional loyalty, civilian activation |
| `faultline-events` | Conditional event triggers, effect application, event chains |
| `faultline-engine` | Deterministic tick loop: events, AI, movement, combat, politics |
| `faultline-stats` | Monte Carlo runner, win probabilities, distribution stats |
| `faultline-backend-wasm` | WASM API (load, validate, run scenarios from browser) |
| `faultline-cli` | Headless CLI with rayon parallelism for batch runs |

## Design Philosophy

- **Config-as-input.** Scenarios are declarative TOML files — shareable, forkable, diffable.
- **Monte Carlo first.** Single runs tell stories. Thousands of runs give distributions.
- **Technology as capability cards.** Named bundles of statistical effects, not simulated hardware.
- **Deterministic.** Same config + same seed = identical output on native and WASM.

## Legal

Faultline is an analytical research tool. All scenario data is derived from publicly available open-source intelligence (OSINT). The software models aggregate statistical effects of military systems — it does not implement, simulate, or contain any controlled defense technology, classified information, or export-restricted algorithms. See [LEGAL.md](LEGAL.md) for full details.

## Security Notice

> **OpenAI/Google integrations are disabled within PR reviews.** OpenAI/Google permits government partners unrestricted use of their models. We only allow models with explicit prohibitions on mass surveillance and autonomous weapons.

## Companion Repository

| Repository | Description |
|------------|-------------|
| [template-repo](https://github.com/AndrewAltimit/template-repo) | Agent orchestration, MCP servers, CI/CD templates, security tooling |

## License

Dual-licensed under [Unlicense](LICENSE) and [MIT](LICENSE-MIT).
