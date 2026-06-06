use serde::{Deserialize, Serialize};
use thiserror::Error;

use faultline_types::ids::TechCardId;
use faultline_types::map::TerrainType;
use faultline_types::tech::{
    MoraleTarget, TechCard, TechCategory, TechEffect, TerrainTechModifier,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during technology effect resolution.
#[derive(Debug, Error)]
pub enum TechError {
    #[error("tech card not found: {0}")]
    CardNotFound(TechCardId),

    #[error("invalid effect configuration: {0}")]
    InvalidEffect(String),

    #[error("conflicting tech requirements: {0}")]
    Conflict(String),
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A resolved tech effect with the final magnitude after terrain
/// modifiers have been applied.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedEffect {
    /// The original effect definition.
    pub effect: TechEffect,
    /// Final effectiveness multiplier in `(0.0, ...]`.
    pub effectiveness: f64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Apply terrain modifiers to a tech card's effects and return the
/// list of resolved effects with adjusted effectiveness.
///
/// Each effect's base value is scaled by the terrain modifier that
/// matches the given terrain type (if any). When no modifier matches,
/// the effectiveness defaults to `1.0`.
pub fn apply_tech_effects(tech: &TechCard, terrain: &TerrainType) -> Vec<ResolvedEffect> {
    // Find the terrain modifier for the current terrain type.
    let terrain_effectiveness = tech
        .terrain_modifiers
        .iter()
        .find(|m| m.terrain == *terrain)
        .map_or(1.0, |m| m.effectiveness);

    tech.effects
        .iter()
        .map(|effect| ResolvedEffect {
            effect: effect.clone(),
            effectiveness: terrain_effectiveness,
        })
        .collect()
}

/// Check whether the given tech card is countered by any of the
/// currently active tech cards.
pub fn is_countered(tech: &TechCard, active_techs: &[TechCardId]) -> bool {
    tech.countered_by
        .iter()
        .any(|counter| active_techs.contains(counter))
}

// ---------------------------------------------------------------------------
// Healthcare & critical-infrastructure capability library
// ---------------------------------------------------------------------------
//
// A coherent, OSINT-grounded set of capability cards covering the
// healthcare / critical-infrastructure cyber-conflict sector, which was
// under-represented in the bundled drone / cyber libraries. Each card is
// a named bundle of `TechEffect`s describing the *aggregate statistical
// effect* of a real-world capability (an attack surface or a defensive
// control), never an implementation. The parameters are plausibly
// sourceable from public OSINT:
//
//   - CISA / HHS HC3 (Health Sector Cybersecurity Coordination Center)
//     public advisories on hospital ransomware and IoMT exposure.
//   - GAO and CRS reports on critical-infrastructure and water/grid SCADA
//     security.
//   - FDA premarket / postmarket medical-device cybersecurity guidance
//     (published, non-technical effect descriptions only).
//   - Published academic and defensive-community studies quantifying
//     hospital downtime, EHR-availability loss, and emergency-services
//     (e.g. 911 / EMS diversion) degradation.
//   - IISS / RAND open analyses of critical-infrastructure resilience.
//
// All figures are published *effects* (downtime in days, % of facilities
// exposed, detection coverage) — no classified, CUI, or export-controlled
// material and no exploit / implementation detail.
//
// These cards are net-new: they are NOT referenced by any existing
// bundled scenario, so adding them cannot change existing scenario
// output or the bundled-verify hashes. New flagship scenarios opt in by
// referencing them in `tech_access`.

/// Build the healthcare / critical-infrastructure capability library.
///
/// Returns the cards in a `Vec` for deterministic iteration. The set is
/// split into offensive exposure/attack cards and defensive control
/// cards; the `countered_by` links wire the two together so a scenario
/// that deploys the matching defensive control degrades the
/// corresponding offensive effect.
pub fn healthcare_infra_library() -> Vec<TechCard> {
    vec![
        // ---- Offensive / exposure cards ---------------------------------
        iomt_exposure_surface(),
        hospital_ransomware_impact(),
        ehr_availability_disruption(),
        scada_ot_exposure(),
        emergency_services_degradation(),
        // ---- Defensive control cards ------------------------------------
        clinical_network_segmentation(),
        medical_device_asset_inventory(),
        offline_ehr_continuity(),
        ot_anomaly_monitoring(),
        emergency_services_continuity(),
    ]
}

/// IoMT (Internet of Medical Things) exposure surface.
///
/// Published surveys report that a large majority of hospital-connected
/// medical devices run unpatched or end-of-life operating systems, and a
/// material fraction carry known-exploited vulnerabilities. Modeled as an
/// intelligence/footholding surface with low defender detection.
pub fn iomt_exposure_surface() -> TechCard {
    TechCard {
        id: TechCardId::from("iomt_exposure_surface"),
        name: "IoMT Device Exposure Surface".into(),
        description: "Aggregate model of the connected-medical-device attack surface: \
            published surveys report a majority of infusion pumps, imaging systems, and \
            patient monitors run unpatched or end-of-life software, with a material \
            fraction carrying known-exploited vulnerabilities. Effect-level only."
            .into(),
        category: TechCategory::Custom("CriticalInfrastructure".into()),
        effects: vec![
            TechEffect::IntelGain { probability: 0.4 },
            TechEffect::DetectionModifier { factor: 0.5 },
        ],
        cost_per_tick: 0.05,
        deployment_cost: 6.0,
        countered_by: vec![
            TechCardId::from("clinical_network_segmentation"),
            TechCardId::from("medical_device_asset_inventory"),
        ],
        terrain_modifiers: vec![TerrainTechModifier {
            terrain: TerrainType::Urban,
            effectiveness: 1.2,
        }],
        coverage_limit: Some(6),
    }
}

/// Hospital-network ransomware impact.
///
/// Public incident reporting and HHS/HC3 advisories put major hospital
/// ransomware outages on the order of two to four weeks of degraded
/// operations, with measurable infrastructure-availability loss and a
/// morale/sentiment hit to the affected population.
pub fn hospital_ransomware_impact() -> TechCard {
    TechCard {
        id: TechCardId::from("hospital_ransomware_impact"),
        name: "Hospital-Network Ransomware Impact".into(),
        description: "Aggregate model of a hospital-network ransomware outage. Public \
            incident reporting puts major outages on the order of 2-4 weeks of degraded \
            operations with significant clinical-system availability loss. Modeled as an \
            infrastructure-availability and civilian-sentiment effect; no payload or \
            intrusion detail."
            .into(),
        category: TechCategory::Cyber,
        effects: vec![
            TechEffect::InfraProtection { factor: 1.8 },
            TechEffect::CivilianSentiment { delta: -0.18 },
            TechEffect::MoraleEffect {
                target: MoraleTarget::Enemy,
                delta: -0.12,
            },
        ],
        cost_per_tick: 0.15,
        deployment_cost: 22.0,
        countered_by: vec![
            TechCardId::from("clinical_network_segmentation"),
            TechCardId::from("offline_ehr_continuity"),
        ],
        terrain_modifiers: vec![TerrainTechModifier {
            terrain: TerrainType::Urban,
            effectiveness: 1.15,
        }],
        coverage_limit: Some(2),
    }
}

/// EHR (electronic health record) availability disruption.
///
/// Loss of EHR availability forces clinicians onto paper workflows;
/// published studies report sharp throughput and decision-support losses
/// during downtime. Modeled as comms/coordination disruption plus a
/// civilian-sentiment hit.
pub fn ehr_availability_disruption() -> TechCard {
    TechCard {
        id: TechCardId::from("ehr_availability_disruption"),
        name: "EHR Availability Disruption".into(),
        description: "Aggregate model of electronic-health-record availability loss \
            forcing clinicians onto paper workflows. Published downtime studies report \
            sharp throughput and decision-support degradation. Modeled as a \
            coordination-disruption and sentiment effect."
            .into(),
        category: TechCategory::Cyber,
        effects: vec![
            TechEffect::CommsDisruption { factor: 0.5 },
            TechEffect::CivilianSentiment { delta: -0.1 },
        ],
        cost_per_tick: 0.1,
        deployment_cost: 14.0,
        countered_by: vec![TechCardId::from("offline_ehr_continuity")],
        terrain_modifiers: vec![],
        coverage_limit: Some(3),
    }
}

/// Water / grid SCADA & OT exposure.
///
/// GAO and CRS reporting and public ICS advisories describe widespread
/// exposure of water-treatment and grid SCADA/OT, including
/// internet-reachable human-machine interfaces and default credentials.
/// Modeled as an interdiction/area-denial surface against the utility's
/// supply function.
pub fn scada_ot_exposure() -> TechCard {
    TechCard {
        id: TechCardId::from("scada_ot_exposure"),
        name: "Water/Grid SCADA Exposure".into(),
        description: "Aggregate model of water-treatment and grid SCADA/OT exposure. \
            Public ICS advisories and GAO/CRS reporting describe internet-reachable \
            human-machine interfaces and weak access controls at a non-trivial fraction \
            of utilities. Modeled as a supply-interdiction and area-denial effect; no \
            ICS protocol or exploit detail."
            .into(),
        category: TechCategory::Custom("CriticalInfrastructure".into()),
        effects: vec![
            TechEffect::SupplyInterdiction { factor: 0.45 },
            TechEffect::AreaDenial { strength: 0.3 },
            TechEffect::IntelGain { probability: 0.3 },
        ],
        cost_per_tick: 0.12,
        deployment_cost: 18.0,
        countered_by: vec![TechCardId::from("ot_anomaly_monitoring")],
        terrain_modifiers: vec![
            TerrainTechModifier {
                terrain: TerrainType::Riverine,
                effectiveness: 1.25,
            },
            TerrainTechModifier {
                terrain: TerrainType::Coastal,
                effectiveness: 1.15,
            },
        ],
        coverage_limit: Some(3),
    }
}

/// Emergency-services (911 / EMS) degradation.
///
/// Public reporting on ransomware and telephony-denial incidents against
/// public-safety answering points documents 911 outages and EMS
/// diversion. Modeled as area denial plus a civilian-sentiment and
/// morale hit.
pub fn emergency_services_degradation() -> TechCard {
    TechCard {
        id: TechCardId::from("emergency_services_degradation"),
        name: "Emergency-Services (911/EMS) Degradation".into(),
        description: "Aggregate model of public-safety answering point and EMS \
            disruption. Public reporting documents 911 outages and ambulance diversion \
            during ransomware and telephony-denial incidents. Modeled as an area-denial \
            and civilian-sentiment effect."
            .into(),
        category: TechCategory::Custom("CriticalInfrastructure".into()),
        effects: vec![
            TechEffect::AreaDenial { strength: 0.4 },
            TechEffect::CivilianSentiment { delta: -0.2 },
            TechEffect::MoraleEffect {
                target: MoraleTarget::Civilian,
                delta: -0.15,
            },
        ],
        cost_per_tick: 0.08,
        deployment_cost: 12.0,
        countered_by: vec![TechCardId::from("emergency_services_continuity")],
        terrain_modifiers: vec![TerrainTechModifier {
            terrain: TerrainType::Urban,
            effectiveness: 1.2,
        }],
        coverage_limit: Some(4),
    }
}

/// Clinical network segmentation (defensive control).
///
/// Segmenting clinical VLANs and isolating IoMT from the enterprise
/// network is the most-cited control for limiting ransomware blast radius
/// and IoMT exploitation. Counters the IoMT-exposure and hospital-
/// ransomware cards.
pub fn clinical_network_segmentation() -> TechCard {
    TechCard {
        id: TechCardId::from("clinical_network_segmentation"),
        name: "Clinical Network Segmentation".into(),
        description: "Defensive control: segmentation of clinical VLANs and isolation of \
            connected medical devices from the enterprise network. The most-cited control \
            for limiting ransomware blast radius and medical-device exploitation."
            .into(),
        category: TechCategory::Custom("CriticalInfrastructure".into()),
        effects: vec![
            TechEffect::CounterTech {
                target: TechCardId::from("iomt_exposure_surface"),
                reduction: 0.5,
            },
            TechEffect::CounterTech {
                target: TechCardId::from("hospital_ransomware_impact"),
                reduction: 0.4,
            },
            TechEffect::InfraProtection { factor: 0.7 },
        ],
        cost_per_tick: 0.07,
        deployment_cost: 16.0,
        countered_by: vec![],
        terrain_modifiers: vec![],
        coverage_limit: None,
    }
}

/// Medical-device asset inventory & lifecycle management (defensive).
///
/// A maintained inventory with patch/lifecycle tracking is the
/// prerequisite control FDA and HHS guidance emphasize; it raises
/// detection of anomalous device behavior and reduces the exploitable
/// IoMT surface.
pub fn medical_device_asset_inventory() -> TechCard {
    TechCard {
        id: TechCardId::from("medical_device_asset_inventory"),
        name: "Medical-Device Asset Inventory".into(),
        description: "Defensive control: a maintained connected-medical-device inventory \
            with patch and lifecycle tracking, as emphasized in published FDA and HHS \
            guidance. Raises detection of anomalous device behavior and shrinks the \
            exploitable surface."
            .into(),
        category: TechCategory::Custom("CriticalInfrastructure".into()),
        effects: vec![
            TechEffect::CounterTech {
                target: TechCardId::from("iomt_exposure_surface"),
                reduction: 0.45,
            },
            TechEffect::DetectionModifier { factor: 1.4 },
        ],
        cost_per_tick: 0.05,
        deployment_cost: 10.0,
        countered_by: vec![],
        terrain_modifiers: vec![],
        coverage_limit: None,
    }
}

/// Offline EHR continuity / downtime procedures (defensive).
///
/// Tested downtime procedures and read-only EHR replicas blunt the
/// availability-loss effect. Counters the EHR-disruption and hospital-
/// ransomware cards.
pub fn offline_ehr_continuity() -> TechCard {
    TechCard {
        id: TechCardId::from("offline_ehr_continuity"),
        name: "Offline EHR Continuity Procedures".into(),
        description: "Defensive control: tested clinical-downtime procedures and \
            read-only EHR replicas that preserve continuity of care during an outage. \
            Blunts electronic-health-record availability loss."
            .into(),
        category: TechCategory::Custom("CriticalInfrastructure".into()),
        effects: vec![
            TechEffect::CounterTech {
                target: TechCardId::from("ehr_availability_disruption"),
                reduction: 0.55,
            },
            TechEffect::CounterTech {
                target: TechCardId::from("hospital_ransomware_impact"),
                reduction: 0.3,
            },
        ],
        cost_per_tick: 0.04,
        deployment_cost: 7.0,
        countered_by: vec![],
        terrain_modifiers: vec![],
        coverage_limit: None,
    }
}

/// OT / SCADA anomaly monitoring (defensive).
///
/// Passive OT-network monitoring with protocol-aware anomaly detection is
/// the headline control in CISA ICS guidance; it raises detection and
/// reduces the SCADA-exposure interdiction effect.
pub fn ot_anomaly_monitoring() -> TechCard {
    TechCard {
        id: TechCardId::from("ot_anomaly_monitoring"),
        name: "OT/SCADA Anomaly Monitoring".into(),
        description: "Defensive control: passive operational-technology network \
            monitoring with protocol-aware anomaly detection, the headline control in \
            published ICS-security guidance. Raises detection and reduces the \
            SCADA-exposure interdiction effect."
            .into(),
        category: TechCategory::Custom("CriticalInfrastructure".into()),
        effects: vec![
            TechEffect::CounterTech {
                target: TechCardId::from("scada_ot_exposure"),
                reduction: 0.5,
            },
            TechEffect::DetectionModifier { factor: 1.5 },
        ],
        cost_per_tick: 0.09,
        deployment_cost: 20.0,
        countered_by: vec![],
        terrain_modifiers: vec![],
        coverage_limit: None,
    }
}

/// Emergency-services continuity / mutual aid (defensive).
///
/// Backup public-safety answering point routing, mutual-aid agreements,
/// and analog fallbacks restore emergency-services capacity. Counters the
/// emergency-services-degradation card.
pub fn emergency_services_continuity() -> TechCard {
    TechCard {
        id: TechCardId::from("emergency_services_continuity"),
        name: "Emergency-Services Continuity & Mutual Aid".into(),
        description: "Defensive control: backup public-safety answering point routing, \
            regional mutual-aid agreements, and analog fallbacks that restore \
            emergency-services capacity during a disruption."
            .into(),
        category: TechCategory::Custom("CriticalInfrastructure".into()),
        effects: vec![
            TechEffect::CounterTech {
                target: TechCardId::from("emergency_services_degradation"),
                reduction: 0.5,
            },
            TechEffect::CivilianSentiment { delta: 0.08 },
        ],
        cost_per_tick: 0.05,
        deployment_cost: 9.0,
        countered_by: vec![],
        terrain_modifiers: vec![],
        coverage_limit: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_card() -> TechCard {
        TechCard {
            id: TechCardId::from("cyber-01"),
            name: "Cyber Disruption Suite".into(),
            description: "Disrupts enemy networks".into(),
            category: TechCategory::Cyber,
            effects: vec![TechEffect::CommsDisruption { factor: 0.6 }],
            cost_per_tick: 2.0,
            deployment_cost: 10.0,
            countered_by: vec![TechCardId::from("firewall-01")],
            terrain_modifiers: vec![
                TerrainTechModifier {
                    terrain: TerrainType::Urban,
                    effectiveness: 1.5,
                },
                TerrainTechModifier {
                    terrain: TerrainType::Desert,
                    effectiveness: 0.5,
                },
            ],
            coverage_limit: Some(3),
        }
    }

    #[test]
    fn terrain_bonus_applies() {
        let card = sample_card();
        let effects = apply_tech_effects(&card, &TerrainType::Urban);
        assert_eq!(effects.len(), 1);
        assert!((effects[0].effectiveness - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn terrain_penalty_applies() {
        let card = sample_card();
        let effects = apply_tech_effects(&card, &TerrainType::Desert);
        assert_eq!(effects.len(), 1);
        assert!((effects[0].effectiveness - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn default_effectiveness_when_no_modifier() {
        let card = sample_card();
        let effects = apply_tech_effects(&card, &TerrainType::Arctic);
        assert_eq!(effects.len(), 1);
        assert!((effects[0].effectiveness - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn is_countered_true_when_active() {
        let card = sample_card();
        let active = vec![TechCardId::from("firewall-01")];
        assert!(is_countered(&card, &active));
    }

    #[test]
    fn is_countered_false_when_not_active() {
        let card = sample_card();
        let active = vec![TechCardId::from("other-99")];
        assert!(!is_countered(&card, &active));
    }

    #[test]
    fn apply_tech_effects_multiple_effects() {
        let card = TechCard {
            id: TechCardId::from("multi-01"),
            name: "Multi-Effect Suite".into(),
            description: "Three distinct effects".into(),
            category: TechCategory::Cyber,
            effects: vec![
                TechEffect::CommsDisruption { factor: 0.5 },
                TechEffect::CombatModifier { factor: 1.2 },
                TechEffect::DetectionModifier { factor: 0.8 },
            ],
            cost_per_tick: 1.0,
            deployment_cost: 5.0,
            countered_by: vec![],
            terrain_modifiers: vec![],
            coverage_limit: None,
        };
        let resolved = apply_tech_effects(&card, &TerrainType::Urban);
        assert_eq!(resolved.len(), 3, "all three effects should be resolved");
        // No terrain modifier configured, so effectiveness should be 1.0.
        for r in &resolved {
            assert!(
                (r.effectiveness - 1.0).abs() < f64::EPSILON,
                "default effectiveness should be 1.0"
            );
        }
    }

    #[test]
    fn apply_tech_effects_empty_effects() {
        let card = TechCard {
            id: TechCardId::from("empty-01"),
            name: "No Effects".into(),
            description: "Card with no effects".into(),
            category: TechCategory::Surveillance,
            effects: vec![],
            cost_per_tick: 0.0,
            deployment_cost: 0.0,
            countered_by: vec![],
            terrain_modifiers: vec![],
            coverage_limit: None,
        };
        let resolved = apply_tech_effects(&card, &TerrainType::Forest);
        assert!(
            resolved.is_empty(),
            "empty effects should return empty resolved list"
        );
    }

    #[test]
    fn is_countered_with_multiple_counters() {
        let card = TechCard {
            id: TechCardId::from("drone-01"),
            name: "Drone Swarm".into(),
            description: "Autonomous drone swarm".into(),
            category: TechCategory::OffensiveDrone,
            effects: vec![TechEffect::CombatModifier { factor: 1.5 }],
            cost_per_tick: 3.0,
            deployment_cost: 20.0,
            countered_by: vec![
                TechCardId::from("counter-uas-01"),
                TechCardId::from("ew-jammer-01"),
            ],
            terrain_modifiers: vec![],
            coverage_limit: None,
        };
        // Only one of the two counters is active.
        let active = vec![
            TechCardId::from("unrelated-99"),
            TechCardId::from("ew-jammer-01"),
        ];
        assert!(
            is_countered(&card, &active),
            "card should be countered when one of its counters is active"
        );

        // Neither counter is active.
        let no_counter = vec![TechCardId::from("unrelated-99")];
        assert!(
            !is_countered(&card, &no_counter),
            "card should not be countered when no counter is active"
        );
    }

    // ----------------------------------------------------------------
    // Healthcare / critical-infrastructure library
    // ----------------------------------------------------------------

    #[test]
    fn healthcare_library_has_expected_size() {
        let lib = healthcare_infra_library();
        assert_eq!(lib.len(), 10, "five offensive + five defensive cards");
    }

    #[test]
    fn healthcare_library_ids_are_unique() {
        let lib = healthcare_infra_library();
        let mut ids: Vec<&str> = lib.iter().map(|c| c.id.0.as_str()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "every card id must be unique");
    }

    #[test]
    fn healthcare_library_cards_have_effects_and_positive_cost() {
        for card in healthcare_infra_library() {
            assert!(
                !card.effects.is_empty(),
                "card {} must carry at least one effect",
                card.id
            );
            assert!(
                card.deployment_cost >= 0.0 && card.cost_per_tick >= 0.0,
                "card {} costs must be non-negative",
                card.id
            );
        }
    }

    /// Every `countered_by` reference and every `CounterTech` target must
    /// resolve to a card that exists in the same library — a dangling
    /// reference is a silent-no-op shape we want to catch at test time.
    #[test]
    fn healthcare_library_counter_links_resolve() {
        let lib = healthcare_infra_library();
        let known: std::collections::BTreeSet<&str> = lib.iter().map(|c| c.id.0.as_str()).collect();

        for card in &lib {
            for counter in &card.countered_by {
                assert!(
                    known.contains(counter.0.as_str()),
                    "card {} is countered_by unknown card {}",
                    card.id,
                    counter
                );
            }
            for effect in &card.effects {
                if let TechEffect::CounterTech { target, .. } = effect {
                    assert!(
                        known.contains(target.0.as_str()),
                        "card {} CounterTech targets unknown card {}",
                        card.id,
                        target
                    );
                }
            }
        }
    }

    /// Each defensive control's `CounterTech` is symmetric with the
    /// offensive card's `countered_by`: if a control reduces an attack,
    /// that attack should list the control as a counter. This keeps the
    /// offense/defense pairing coherent.
    #[test]
    fn healthcare_library_counters_are_symmetric() {
        let lib = healthcare_infra_library();
        for card in &lib {
            for effect in &card.effects {
                if let TechEffect::CounterTech { target, .. } = effect {
                    let attacked = lib
                        .iter()
                        .find(|c| c.id == *target)
                        .expect("CounterTech target must exist in the library");
                    assert!(
                        attacked.countered_by.contains(&card.id),
                        "control {} reduces {} but {} does not list it as a counter",
                        card.id,
                        target,
                        target
                    );
                }
            }
        }
    }

    #[test]
    fn scada_exposure_gets_riverine_terrain_bonus() {
        let card = scada_ot_exposure();
        let resolved = apply_tech_effects(&card, &TerrainType::Riverine);
        assert!(!resolved.is_empty());
        assert!(
            (resolved[0].effectiveness - 1.25).abs() < f64::EPSILON,
            "SCADA exposure should be more effective in riverine terrain"
        );
    }

    #[test]
    fn ot_monitoring_counters_scada_exposure() {
        let attack = scada_ot_exposure();
        let active = vec![TechCardId::from("ot_anomaly_monitoring")];
        assert!(
            is_countered(&attack, &active),
            "OT anomaly monitoring should counter SCADA exposure"
        );
    }
}
