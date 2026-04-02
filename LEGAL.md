# Legal Notice

## Analytical Tool — Not a Weapons System

Faultline is an analytical tool for conflict simulation research. It is a statistical modeling engine that processes user-supplied scenario configurations and produces probability distributions of outcomes. It does not implement, simulate, or contain any controlled defense technology, classified information, or export-restricted algorithms.

## Open-Source Methodology

All scenario data included with this project, and all scenario data the engine is designed to consume, is derived from publicly available open-source intelligence (OSINT) including but not limited to:

- **IISS Military Balance** — annual assessment of global military capabilities
- **Congressional Research Service (CRS) reports** — published analyses of defense programs and force structure
- **RAND Corporation publications** — unclassified wargame analyses and force assessment methodologies
- **Department of Defense budget justifications** — publicly released program descriptions and capability summaries
- **GlobalFirepower and similar OSINT aggregators** — compiled open-source order of battle data
- **Academic publications** — peer-reviewed operations research, Lanchester modeling, and game theory literature
- **Congressional testimony and GAO reports** — public statements on system capabilities and program status

## What This Software Does

Faultline models the **aggregate statistical effects** of military systems, political dynamics, and force compositions. Technology capabilities are represented as named parameter bundles (e.g., detection probability, combat effectiveness modifier, communication disruption factor) derived from publicly cited performance characteristics.

Examples of what a Faultline tech card contains:

- "System X has a detection range of 300km against 1m² RCS targets" (from published specifications)
- "System Y has P(kill) 0.85 against theater ballistic missiles" (from congressional testimony)
- "Platform Z reduces sensor-to-shooter latency from 20 minutes to 3 minutes" (from DOD program descriptions)

These are floating-point parameters that describe the *effects* of capabilities on simulation outcomes. They are not designs, implementations, algorithms, waveforms, or technical data for any defense system.

## What This Software Does Not Contain

- No cryptographic implementations (no ITAR Category XIII items)
- No guidance, navigation, or control algorithms (no EAR Category 7)
- No sensor signal processing, radar waveform design, or EW implementations (no EAR Category 11)
- No autonomous targeting or fire control logic
- No classified or controlled unclassified information (CUI)
- No defense technical data as defined under ITAR § 120.33
- No operational planning tools that interface with real-world C2 systems

## Precedent

The methodology employed by Faultline — open-source OOB data, parameterized weapon effectiveness curves, and aggregate Lanchester-family combat models — is standard practice in published defense analysis. Comparable approaches are used in:

- RAND Corporation published wargame frameworks
- US Army War College educational simulations
- NATO unclassified analytical wargames
- Academic operations research literature
- Commercial conflict simulation software (e.g., MANA, STORM, JANUS)

## Disclaimer

Faultline is not a predictive model. It does not forecast real-world outcomes. It is a what-if engine that explores the consequence space of user-defined assumptions. The user supplies the premises; the engine enforces internal consistency and produces statistical distributions.

No warranty is made regarding the accuracy, completeness, or applicability of any scenario data or simulation output. Users are responsible for ensuring their use of this software complies with all applicable laws and regulations in their jurisdiction.
