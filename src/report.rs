use crate::sim::{
    ActionRejection, AgentId, ConfrontationOutcome, DeathCause, DialogueTone, EventKind,
    LocationId, NEW_WORLD_START_HOUR, Offering, Tick, World,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub run: RunMetrics,
    pub residents: Vec<ResidentMetrics>,
    pub social: SocialMetrics,
    pub economy: EconomyMetrics,
    pub behaviour: BehaviourMetrics,
    pub llm: LlmMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMetrics {
    pub ticks: u64,
    pub events: u64,
    pub deaths_by_cause: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResidentMetrics {
    pub id: AgentId,
    pub name: String,
    pub balance: u64,
    pub health: f32,
    pub mood: f32,
    pub relationship_count: u64,
    pub relationship_mean: Option<f32>,
    pub relationship_min: Option<f32>,
    pub relationship_max: Option<f32>,
    pub memories: u64,
    pub rumors_carried: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResidentTalks {
    pub id: AgentId,
    pub name: String,
    pub talks: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocialMetrics {
    pub unique_conversation_pairs: u64,
    pub talks_per_resident: Vec<ResidentTalks>,
    pub tone_distribution: BTreeMap<String, u64>,
    pub rumor_max_depth: u8,
    pub confrontations_by_outcome: BTreeMap<String, u64>,
    pub aid_given: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BusinessMetrics {
    pub id: LocationId,
    pub name: String,
    pub offering: Offering,
    pub cash: u64,
    pub stock: u32,
    pub revenue: u64,
    pub wages_paid: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomyMetrics {
    pub businesses: Vec<BusinessMetrics>,
    pub insolvent_employer_rejections: u64,
    pub purchases_by_offering: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BehaviourMetrics {
    pub goals_completed: u64,
    pub rejected_actions_by_reason: BTreeMap<String, u64>,
    pub waited_events: u64,
    pub waited_share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LlmMetrics {
    pub attempts: u64,
    pub decisions: u64,
    pub fallbacks: u64,
    pub intentions_started: u64,
    pub intentions_completed: u64,
    pub intentions_interrupted: u64,
}

impl Report {
    pub fn from_world(world: &World) -> Self {
        let mut deaths_by_cause = BTreeMap::new();
        let mut talks = BTreeMap::<AgentId, u64>::new();
        let mut conversation_pairs = BTreeSet::new();
        let mut tone_distribution = BTreeMap::new();
        let mut confrontations_by_outcome = BTreeMap::new();
        let mut purchases_by_offering = BTreeMap::new();
        let mut rejected_actions_by_reason = BTreeMap::new();
        let mut aid_given = 0;
        let mut insolvent_employer_rejections = 0;
        let mut goals_completed = 0;
        let mut waited_events = 0;

        for event in world.events() {
            match &event.kind {
                EventKind::Spoke {
                    speaker,
                    listener,
                    tone,
                    ..
                } => {
                    *talks.entry(*speaker).or_default() += 1;
                    *talks.entry(*listener).or_default() += 1;
                    conversation_pairs.insert(if speaker < listener {
                        (*speaker, *listener)
                    } else {
                        (*listener, *speaker)
                    });
                    increment(&mut tone_distribution, tone_name(*tone));
                }
                EventKind::Confronted { outcome, .. } => {
                    increment(&mut confrontations_by_outcome, confrontation_name(*outcome));
                }
                EventKind::Purchased { offering, .. } => {
                    increment(&mut purchases_by_offering, offering_name(*offering));
                }
                EventKind::ItemGiven { .. } => aid_given += 1,
                EventKind::GoalCompleted { .. } => goals_completed += 1,
                EventKind::Waited { .. } => waited_events += 1,
                EventKind::Died { cause, .. } => {
                    increment(&mut deaths_by_cause, death_cause_name(*cause));
                }
                EventKind::ActionRejected { reason, .. } => {
                    increment(&mut rejected_actions_by_reason, rejection_name(reason));
                    if matches!(reason, ActionRejection::InsolventEmployer { .. }) {
                        insolvent_employer_rejections += 1;
                    }
                }
                EventKind::Stole { .. }
                | EventKind::TheftFailed { .. }
                | EventKind::Robbed { .. } => {}
                EventKind::TownEventStarted { .. }
                | EventKind::TownEventEnded { .. }
                | EventKind::Moved { .. }
                | EventKind::Observed { .. }
                | EventKind::ItemUsed { .. }
                | EventKind::Treated { .. }
                | EventKind::Rested { .. }
                | EventKind::Worked { .. }
                | EventKind::DiseaseInfected { .. }
                | EventKind::DiseaseSymptoms { .. }
                | EventKind::DiseaseRecovered { .. }
                | EventKind::DiseaseImmunityExpired { .. } => {}
            }
        }

        let residents = world
            .agents
            .values()
            .map(|agent| {
                let scores = agent
                    .relationships
                    .values()
                    .map(|relationship| relationship.score())
                    .collect::<Vec<_>>();
                ResidentMetrics {
                    id: agent.id,
                    name: agent.name.clone(),
                    balance: agent.balance,
                    health: agent.health,
                    mood: agent.mood,
                    relationship_count: scores.len() as u64,
                    relationship_mean: (!scores.is_empty())
                        .then(|| scores.iter().sum::<f32>() / scores.len() as f32),
                    relationship_min: scores.iter().copied().reduce(f32::min),
                    relationship_max: scores.iter().copied().reduce(f32::max),
                    memories: agent.memories.len() as u64,
                    rumors_carried: agent.rumors.len() as u64,
                }
            })
            .collect();
        let talks_per_resident = world
            .agents
            .values()
            .map(|agent| ResidentTalks {
                id: agent.id,
                name: agent.name.clone(),
                talks: talks.get(&agent.id).copied().unwrap_or_default(),
            })
            .collect();
        let businesses = world
            .locations
            .values()
            .filter_map(|location| {
                location.business.map(|business| BusinessMetrics {
                    id: location.id,
                    name: location.name.clone(),
                    offering: business.offering,
                    cash: business.cash,
                    stock: business.stock,
                    revenue: business.revenue,
                    wages_paid: business.wages_paid,
                })
            })
            .collect();
        let llm = world.agents.values().map(|agent| agent.routing).fold(
            LlmMetrics::default(),
            |mut total, stats| {
                total.decisions += stats.llm_decisions;
                total.fallbacks += stats.llm_fallbacks;
                total.attempts += stats.llm_decisions + stats.llm_fallbacks;
                total.intentions_started += stats.llm_intentions_started;
                total.intentions_completed += stats.llm_intentions_completed;
                total.intentions_interrupted += stats.llm_intentions_interrupted;
                total
            },
        );

        Self {
            run: RunMetrics {
                ticks: world
                    .tick
                    .0
                    .saturating_sub(NEW_WORLD_START_HOUR * 60 / Tick::MINUTES),
                events: world.events().len() as u64,
                deaths_by_cause,
            },
            residents,
            social: SocialMetrics {
                unique_conversation_pairs: conversation_pairs.len() as u64,
                talks_per_resident,
                tone_distribution,
                rumor_max_depth: world
                    .agents
                    .values()
                    .flat_map(|agent| &agent.rumors)
                    .map(|rumor| rumor.depth)
                    .max()
                    .unwrap_or_default(),
                confrontations_by_outcome,
                aid_given,
            },
            economy: EconomyMetrics {
                businesses,
                insolvent_employer_rejections,
                purchases_by_offering,
            },
            behaviour: BehaviourMetrics {
                goals_completed,
                rejected_actions_by_reason,
                waited_events,
                waited_share: if world.events().is_empty() {
                    0.0
                } else {
                    waited_events as f64 / world.events().len() as f64
                },
            },
            llm,
        }
    }

    pub fn render_table(&self) -> String {
        let mut lines = vec![
            "=== RUN ===".into(),
            format!("Ticks                 {}", self.run.ticks),
            format!("Events                {}", self.run.events),
            format!(
                "Deaths by cause       {}",
                render_counts(&self.run.deaths_by_cause)
            ),
            "".into(),
            "=== RESIDENTS ===".into(),
            "Resident                 Balance Health  Mood   Relations Mean   Min    Max    Memories Rumors".into(),
        ];
        for resident in &self.residents {
            lines.push(format!(
                "{:<24} {:>7} {:>6.2} {:>6.2} {:>9} {:>6} {:>6} {:>6} {:>8} {:>6}",
                resident.name,
                resident.balance,
                resident.health,
                resident.mood,
                resident.relationship_count,
                render_score(resident.relationship_mean),
                render_score(resident.relationship_min),
                render_score(resident.relationship_max),
                resident.memories,
                resident.rumors_carried,
            ));
        }
        lines.extend([
            "".into(),
            "=== SOCIAL ===".into(),
            format!(
                "Unique conversation pairs  {}",
                self.social.unique_conversation_pairs
            ),
            format!(
                "Talks per resident         {}",
                self.social
                    .talks_per_resident
                    .iter()
                    .map(|resident| format!("{}={}", resident.name, resident.talks))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            format!(
                "Tone distribution          {}",
                render_counts(&self.social.tone_distribution)
            ),
            format!("Rumor max depth           {}", self.social.rumor_max_depth),
            format!(
                "Confrontations             {}",
                render_counts(&self.social.confrontations_by_outcome)
            ),
            format!("Aid given                  {}", self.social.aid_given),
            "".into(),
            "=== ECONOMY ===".into(),
            "Business                  Offering        Cash Stock Revenue Wages".into(),
        ]);
        for business in &self.economy.businesses {
            lines.push(format!(
                "{:<25} {:<15} {:>4} {:>5} {:>7} {:>5}",
                business.name,
                business.offering,
                business.cash,
                business.stock,
                business.revenue,
                business.wages_paid,
            ));
        }
        lines.extend([
            format!(
                "Insolvent employers        {}",
                self.economy.insolvent_employer_rejections
            ),
            format!(
                "Purchases by offering      {}",
                render_counts(&self.economy.purchases_by_offering)
            ),
            "".into(),
            "=== BEHAVIOUR ===".into(),
            format!(
                "Goals completed            {}",
                self.behaviour.goals_completed
            ),
            format!(
                "Rejected actions           {}",
                render_counts(&self.behaviour.rejected_actions_by_reason)
            ),
            format!(
                "Waited share               {}/{} ({:.1}%)",
                self.behaviour.waited_events,
                self.run.events,
                self.behaviour.waited_share * 100.0
            ),
            "".into(),
            "=== LLM ===".into(),
            format!("Attempts                   {}", self.llm.attempts),
            format!("Decisions                  {}", self.llm.decisions),
            format!("Fallbacks                  {}", self.llm.fallbacks),
            format!("Intentions started         {}", self.llm.intentions_started),
            format!(
                "Intentions completed       {}",
                self.llm.intentions_completed
            ),
            format!(
                "Intentions interrupted     {}",
                self.llm.intentions_interrupted
            ),
        ]);
        lines.join("\n")
    }
}

fn increment(counts: &mut BTreeMap<String, u64>, name: &'static str) {
    *counts.entry(name.into()).or_default() += 1;
}

fn render_counts(counts: &BTreeMap<String, u64>) -> String {
    if counts.is_empty() {
        "none".into()
    } else {
        counts
            .iter()
            .map(|(name, count)| format!("{name}={count}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn render_score(score: Option<f32>) -> String {
    score.map_or_else(|| "-".into(), |score| format!("{score:.2}"))
}

fn death_cause_name(cause: DeathCause) -> &'static str {
    match cause {
        DeathCause::Starvation => "starvation",
        DeathCause::Exhaustion => "exhaustion",
        DeathCause::Injury => "injury",
        DeathCause::Disease => "disease",
    }
}

fn tone_name(tone: DialogueTone) -> &'static str {
    match tone {
        DialogueTone::Friendly => "friendly",
        DialogueTone::Supportive => "supportive",
        DialogueTone::Neutral => "neutral",
        DialogueTone::Tense => "tense",
    }
}

fn confrontation_name(outcome: ConfrontationOutcome) -> &'static str {
    match outcome {
        ConfrontationOutcome::Confirmed => "confirmed",
        ConfrontationOutcome::Denied => "denied",
        ConfrontationOutcome::Challenged => "challenged",
    }
}

fn offering_name(offering: Offering) -> &'static str {
    match offering {
        Offering::Meal => "meal",
        Offering::Supplies => "supplies",
        Offering::Repairs => "repairs",
        Offering::Medicine => "medicine",
        Offering::CivicServices => "civic_services",
    }
}

fn rejection_name(reason: &ActionRejection) -> &'static str {
    match reason {
        ActionRejection::UnknownActor(_) => "unknown_actor",
        ActionRejection::UnknownAgent(_) => "unknown_agent",
        ActionRejection::AgentDead(_) => "agent_dead",
        ActionRejection::UnknownLocation(_) => "unknown_location",
        ActionRejection::Disconnected { .. } => "disconnected",
        ActionRejection::SelfTarget(_) => "self_target",
        ActionRejection::NotCoLocated { .. } => "not_co_located",
        ActionRejection::LocationNotVisible { .. } => "location_not_visible",
        ActionRejection::LocationClosed(_) => "location_closed",
        ActionRejection::NoRoute { .. } => "no_route",
        ActionRejection::CannotPurchaseHere(_) => "cannot_purchase_here",
        ActionRejection::SoldOut(_) => "sold_out",
        ActionRejection::InsufficientFunds { .. } => "insufficient_funds",
        ActionRejection::InventoryFull(_) => "inventory_full",
        ActionRejection::ItemUnavailable(_) => "item_unavailable",
        ActionRejection::ItemNotNeeded { .. } => "item_not_needed",
        ActionRejection::EconomyOverflow => "economy_overflow",
        ActionRejection::InsolventEmployer { .. } => "insolvent_employer",
        ActionRejection::CannotSeekTreatmentHere(_) => "cannot_seek_treatment_here",
        ActionRejection::NoMedicalNeed => "no_medical_need",
        ActionRejection::CannotRestHere(_) => "cannot_rest_here",
        ActionRejection::CannotWorkHere(_) => "cannot_work_here",
        ActionRejection::TooUnwell => "too_unwell",
        ActionRejection::OutsideWorkingHours(_) => "outside_working_hours",
        ActionRejection::UnknownClaim(_) => "unknown_claim",
        ActionRejection::ClaimNotAboutTarget { .. } => "claim_not_about_target",
        ActionRejection::EmptyMessage => "empty_message",
        ActionRejection::InvalidMessage => "invalid_message",
        ActionRejection::MessageTooLong { .. } => "message_too_long",
        ActionRejection::LootNotOwned { .. } => "loot_not_owned",
        ActionRejection::InvalidMembership(_) => "invalid_membership",
    }
}

#[cfg(test)]
mod tests {
    use super::Report;
    use crate::{
        decision::LocalDecisionEngine,
        persistence::{load_world, save_world},
        runner::run_simulation,
        sim::World,
    };
    use std::{fs, time::SystemTime};

    #[tokio::test]
    async fn report_round_trips_and_survives_resume() {
        let seed = 814_921;
        let mut engine = LocalDecisionEngine::new(seed);
        let world = run_simulation(World::briar_glen(seed).expect("town"), 24, &mut engine)
            .await
            .expect("run");
        let report = Report::from_world(&world);

        for heading in [
            "=== RUN ===",
            "=== RESIDENTS ===",
            "=== SOCIAL ===",
            "=== ECONOMY ===",
            "=== BEHAVIOUR ===",
            "=== LLM ===",
        ] {
            assert!(report.render_table().contains(heading));
        }
        let json = serde_json::to_string(&report).expect("serialize report");
        assert_eq!(
            serde_json::from_str::<Report>(&json).expect("deserialize report"),
            report
        );

        let path = std::env::temp_dir().join(format!(
            "terrarium-report-{:?}.sqlite",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        save_world(&path, &world).expect("save checkpoint");
        let loaded = load_world(&path).expect("load checkpoint");
        assert_eq!(Report::from_world(&loaded), report);

        let mut resumed_engine = LocalDecisionEngine::new(seed);
        let resumed = run_simulation(loaded, 12, &mut resumed_engine)
            .await
            .expect("resume");
        assert!(Report::from_world(&resumed).run.ticks > report.run.ticks);
        fs::remove_file(path).expect("remove checkpoint");
    }
}
