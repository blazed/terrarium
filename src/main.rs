use std::path::PathBuf;
use terrarium::{
    decision::RandomDecisionEngine,
    observer::render_run,
    persistence::{StoredRun, load_run, save_world},
    runner::run_simulation,
    sim::{Tick, World},
};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

#[derive(Debug, PartialEq, Eq)]
struct RunArgs {
    seed: u64,
    ticks: u64,
    database: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run(RunArgs),
    Inspect(PathBuf),
}

#[derive(Debug, Error, PartialEq, Eq)]
enum CliError {
    #[error(
        "usage: terrarium run [--seed N] [--days N | --ticks N] [--database PATH]\n       terrarium inspect PATH"
    )]
    Usage,
    #[error("missing value for {0}")]
    MissingValue(String),
    #[error("invalid value for {flag}: {value}")]
    InvalidNumber { flag: String, value: String },
    #[error("--days and --ticks cannot be used together")]
    ConflictingDuration,
    #[error("duration must be greater than zero")]
    ZeroDuration,
    #[error("duration is too large")]
    DurationOverflow,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, CliError> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("run") => parse_run_args(args).map(Command::Run),
        Some("inspect") => {
            let path = args.next().ok_or(CliError::Usage)?;
            if args.next().is_some() {
                return Err(CliError::Usage);
            }
            Ok(Command::Inspect(path.into()))
        }
        _ => Err(CliError::Usage),
    }
}

fn parse_run_args(mut args: impl Iterator<Item = String>) -> Result<RunArgs, CliError> {
    let mut seed = 814_921;
    let mut days = None;
    let mut ticks = None;
    let mut database = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| CliError::MissingValue(flag.clone()))?;
        match flag.as_str() {
            "--seed" => seed = parse_number(&flag, value)?,
            "--days" => days = Some(parse_number(&flag, value)?),
            "--ticks" => ticks = Some(parse_number(&flag, value)?),
            "--database" => database = Some(value.into()),
            _ => return Err(CliError::Usage),
        }
    }

    if days.is_some() && ticks.is_some() {
        return Err(CliError::ConflictingDuration);
    }
    let ticks = match (days, ticks) {
        (Some(days), None) => days
            .checked_mul(Tick::PER_DAY)
            .ok_or(CliError::DurationOverflow)?,
        (None, Some(ticks)) => ticks,
        (None, None) => Tick::PER_DAY,
        (Some(_), Some(_)) => return Err(CliError::ConflictingDuration),
    };
    if ticks == 0 {
        return Err(CliError::ZeroDuration);
    }
    Ok(RunArgs {
        seed,
        ticks,
        database,
    })
}

fn parse_number(flag: &str, value: String) -> Result<u64, CliError> {
    value.parse().map_err(|_| CliError::InvalidNumber {
        flag: flag.into(),
        value,
    })
}

fn render_stored_run(run: &StoredRun) -> Result<String, serde_json::Error> {
    let mut lines = vec![
        run.name.clone(),
        format!("Seed: {}", run.seed),
        format!("Elapsed: {}", run.tick),
        format!("Agents: {}", run.agents.len()),
        format!("Locations: {}", run.locations.len()),
        format!("Events: {}", run.events.len()),
        String::new(),
    ];
    for event in &run.events {
        lines.push(serde_json::to_string(event)?);
    }
    Ok(lines.join("\n"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .try_init()?;
    match parse_args(std::env::args().skip(1))? {
        Command::Run(args) => {
            let world = World::briar_glen(args.seed)?;
            let mut engine = RandomDecisionEngine::new(args.seed);
            let world = run_simulation(world, args.ticks, &mut engine).await?;
            if let Some(path) = args.database {
                save_world(path, &world)?;
            }
            println!("{}", render_run(&world));
        }
        Command::Inspect(path) => println!("{}", render_stored_run(&load_run(path)?)?),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CliError, Command, RunArgs, parse_args};
    use std::path::PathBuf;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn parses_run_and_inspect_commands() {
        assert_eq!(
            parse_args(args(&[
                "run",
                "--seed",
                "1234",
                "--days",
                "30",
                "--database",
                "run.sqlite",
            ])),
            Ok(Command::Run(RunArgs {
                seed: 1_234,
                ticks: 8_640,
                database: Some(PathBuf::from("run.sqlite")),
            }))
        );
        assert_eq!(
            parse_args(args(&["inspect", "run.sqlite"])),
            Ok(Command::Inspect(PathBuf::from("run.sqlite")))
        );
    }

    #[test]
    fn defaults_and_conflicting_durations_are_checked() {
        assert_eq!(
            parse_args(args(&["run", "--ticks", "10000"])),
            Ok(Command::Run(RunArgs {
                seed: 814_921,
                ticks: 10_000,
                database: None,
            }))
        );
        assert_eq!(
            parse_args(args(&["run", "--days", "1", "--ticks", "2"])),
            Err(CliError::ConflictingDuration)
        );
    }
}
