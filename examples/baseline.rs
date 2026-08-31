//! Multi-seed baseline: run the local engine for several seeds and print the
//! `report` table per seed plus mean/min/max per metric.
//!
//! Usage: baseline [--seeds N] [--days N]

use std::{process::ExitCode, time::Instant};
use terrarium::{
    decision::LocalDecisionEngine, report::Report, runner::run_simulation, sim::EventKind,
    sim::Tick, sim::World,
};

/// ponytail: ad-hoc counts until #18 formalizes CrimeMetrics in the report;
/// move these rows into Report::from_world then.
fn crime_counts(world: &World) -> (u64, u64) {
    let mut thefts = 0;
    let mut assaults = 0;
    for event in world.events() {
        match event.kind {
            EventKind::Stole { .. } => thefts += 1,
            EventKind::Assaulted { .. } => assaults += 1,
            _ => {}
        }
    }
    (thefts, assaults)
}

struct Args {
    seeds: u64,
    days: u64,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut seeds = 10;
    let mut days = 7;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--seeds" => {
                seeds = value
                    .parse()
                    .map_err(|_| format!("invalid --seeds: {value}"))?
            }
            "--days" => {
                days = value
                    .parse()
                    .map_err(|_| format!("invalid --days: {value}"))?
            }
            _ => return Err(format!("unknown flag: {flag}")),
        }
    }
    if seeds == 0 {
        return Err("--seeds must be greater than zero".into());
    }
    if days == 0 {
        return Err("--days must be greater than zero".into());
    }
    Ok(Args { seeds, days })
}

struct Row {
    name: &'static str,
    ratio: bool,
    values: Vec<f64>,
}

impl Row {
    fn new(name: &'static str, ratio: bool) -> Self {
        Self {
            name,
            ratio,
            values: Vec::new(),
        }
    }
}

fn print_aggregate(rows: &[Row]) {
    println!("\n=== Aggregate (mean | min | max) ===");
    println!("{:<28} {:>9} {:>9} {:>9}", "Metric", "Mean", "Min", "Max");
    for row in rows {
        let mean = row.values.iter().sum::<f64>() / row.values.len() as f64;
        let min = row.values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = row.values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if row.ratio {
            println!("{:<28} {:>9.2} {:>9.2} {:>9.2}", row.name, mean, min, max);
        } else {
            println!("{:<28} {:>9.1} {:>9.0} {:>9.0}", row.name, mean, min, max);
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("baseline: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args(std::env::args().skip(1))?;
    let started = Instant::now();
    let ticks = args.days * Tick::PER_DAY;

    let mut rows = vec![
        Row::new("Events", false),
        Row::new("Deaths", false),
        Row::new("Unique conversation pairs", false),
        Row::new("Talks", false),
        Row::new("Confrontations", false),
        Row::new("Aid given", false),
        Row::new("Rumor max depth", false),
        Row::new("Purchases", false),
        Row::new("Insolvent employers", false),
        Row::new("Goals completed", false),
        Row::new("Rejected actions", false),
        Row::new("Waited share %", true),
        Row::new("Crime: thefts", false),
        Row::new("Crime: assaults", false),
        Row::new("Resident balance", false),
        Row::new("Resident health", false),
        Row::new("Resident mood", false),
        Row::new("Resident relationship mean", false),
        Row::new("Resident memories", false),
        Row::new("Resident rumors carried", false),
    ];

    for seed in 0..args.seeds {
        let world = World::briar_glen(seed)?;
        let mut engine = LocalDecisionEngine::new(seed);
        let world = run_simulation(world, ticks, &mut engine).await?;
        let report = Report::from_world(&world);

        println!("\nSeed {seed}");
        println!("{}", report.render_table());

        let sum =
            |values: &std::collections::BTreeMap<String, u64>| -> u64 { values.values().sum() };
        rows[0].values.push(report.run.events as f64);
        rows[1].values.push(sum(&report.run.deaths_by_cause) as f64);
        rows[2]
            .values
            .push(report.social.unique_conversation_pairs as f64);
        rows[3].values.push(
            report
                .social
                .talks_per_resident
                .iter()
                .map(|resident| resident.talks as f64)
                .sum(),
        );
        rows[4]
            .values
            .push(sum(&report.social.confrontations_by_outcome) as f64);
        rows[5].values.push(report.social.aid_given as f64);
        rows[6].values.push(report.social.rumor_max_depth as f64);
        rows[7]
            .values
            .push(sum(&report.economy.purchases_by_offering) as f64);
        rows[8]
            .values
            .push(report.economy.insolvent_employer_rejections as f64);
        rows[9].values.push(report.behaviour.goals_completed as f64);
        rows[10]
            .values
            .push(sum(&report.behaviour.rejected_actions_by_reason) as f64);
        rows[11].values.push(report.behaviour.waited_share * 100.0);

        let (thefts, assaults) = crime_counts(&world);
        rows[12].values.push(thefts as f64);
        rows[13].values.push(assaults as f64);

        for resident in &report.residents {
            rows[14].values.push(resident.balance as f64);
            rows[15].values.push(resident.health as f64);
            rows[16].values.push(resident.mood as f64);
            if let Some(relationship_mean) = resident.relationship_mean {
                rows[17].values.push(relationship_mean as f64);
            }
            rows[18].values.push(resident.memories as f64);
            rows[19].values.push(resident.rumors_carried as f64);
        }
    }

    println!(
        "\n=== Baseline: {} seeds × {} days ({:.1}s) ===",
        args.seeds,
        args.days,
        started.elapsed().as_secs_f64()
    );
    print_aggregate(&rows);
    Ok(())
}
