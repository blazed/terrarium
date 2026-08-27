use std::{path::PathBuf, time::Duration};
use terrarium::{
    decision::{OpenAiDecisionEngine, RandomDecisionEngine},
    observer::{render_event, render_run_since, render_summary},
    persistence::{StoredRun, load_run, load_world, save_world},
    runner::{run_simulation, run_simulation_with_events},
    sim::{Tick, World},
};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

#[derive(Debug, PartialEq, Eq)]
struct RunArgs {
    seed: u64,
    ticks: u64,
    database: Option<PathBuf>,
    resume: Option<PathBuf>,
    decision: DecisionArgs,
}

#[derive(Debug, PartialEq, Eq)]
enum DecisionArgs {
    Random,
    OpenAi {
        model: String,
        base_url: String,
        api_key_env: Option<String>,
    },
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Run(RunArgs),
    Inspect(PathBuf),
}

#[derive(Debug, Error, PartialEq, Eq)]
enum CliError {
    #[error(
        "usage: terrarium run [--seed N | --resume PATH] [--days N | --ticks N] [--database PATH] [--llm-model MODEL [--llm-url URL] [--llm-api-key-env NAME]]\n       terrarium inspect PATH"
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
    #[error("LLM options require --llm-model")]
    MissingLlmModel,
    #[error("--seed cannot be used with --resume")]
    SeedWithResume,
    #[error("environment variable {0} does not contain an API key")]
    MissingApiKey(String),
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
    let mut seed_was_set = false;
    let mut days = None;
    let mut ticks = None;
    let mut database = None;
    let mut resume = None;
    let mut llm_model = None;
    let mut llm_url = None;
    let mut llm_api_key_env = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| CliError::MissingValue(flag.clone()))?;
        match flag.as_str() {
            "--seed" => {
                seed = parse_number(&flag, value)?;
                seed_was_set = true;
            }
            "--days" => days = Some(parse_number(&flag, value)?),
            "--ticks" => ticks = Some(parse_number(&flag, value)?),
            "--database" => database = Some(value.into()),
            "--resume" => resume = Some(value.into()),
            "--llm-model" => llm_model = Some(value),
            "--llm-url" => llm_url = Some(value),
            "--llm-api-key-env" => llm_api_key_env = Some(value),
            _ => return Err(CliError::Usage),
        }
    }

    if days.is_some() && ticks.is_some() {
        return Err(CliError::ConflictingDuration);
    }
    if seed_was_set && resume.is_some() {
        return Err(CliError::SeedWithResume);
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
    let decision = match llm_model {
        Some(model) => DecisionArgs::OpenAi {
            model,
            base_url: llm_url.unwrap_or_else(|| "http://localhost:11434/v1".into()),
            api_key_env: llm_api_key_env,
        },
        None if llm_url.is_some() || llm_api_key_env.is_some() => {
            return Err(CliError::MissingLlmModel);
        }
        None => DecisionArgs::Random,
    };
    Ok(RunArgs {
        seed,
        ticks,
        database,
        resume,
        decision,
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
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .try_init()?;
    match parse_args(std::env::args().skip(1))? {
        Command::Run(args) => {
            let RunArgs {
                seed,
                ticks,
                database,
                resume,
                decision,
            } = args;
            let world = match &resume {
                Some(path) => load_world(path)?,
                None => World::briar_glen(seed)?,
            };
            let world_seed = world.seed;
            let first_event = world.events().len();
            let (world, streamed) = match decision {
                DecisionArgs::Random => {
                    let mut engine = RandomDecisionEngine::new(world_seed);
                    (run_simulation(world, ticks, &mut engine).await?, false)
                }
                DecisionArgs::OpenAi {
                    model,
                    base_url,
                    api_key_env,
                } => {
                    let mut engine =
                        OpenAiDecisionEngine::new(&base_url, model, Duration::from_secs(120))?;
                    if let Some(name) = api_key_env {
                        let key = std::env::var(&name)
                            .map_err(|_| CliError::MissingApiKey(name.clone()))?;
                        engine = engine
                            .with_api_key(key)
                            .map_err(|_| CliError::MissingApiKey(name))?;
                    }
                    println!(
                        "{}\nSeed: {}\nAgents: {}\n",
                        world.name,
                        world.seed,
                        world.agents.len()
                    );
                    (
                        run_simulation_with_events(world, ticks, &mut engine, |world, event| {
                            println!("{}", render_event(world, event));
                        })
                        .await?,
                        true,
                    )
                }
            };
            if let Some(path) = database.or(resume) {
                save_world(path, &world)?;
            }
            if streamed {
                println!("\n{}", render_summary(&world));
            } else {
                println!("{}", render_run_since(&world, first_event));
            }
        }
        Command::Inspect(path) => println!("{}", render_stored_run(&load_run(path)?)?),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CliError, Command, DecisionArgs, RunArgs, parse_args};
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
                resume: None,
                decision: DecisionArgs::Random,
            }))
        );
        assert_eq!(
            parse_args(args(&["inspect", "run.sqlite"])),
            Ok(Command::Inspect(PathBuf::from("run.sqlite")))
        );
        assert_eq!(
            parse_args(args(&[
                "run",
                "--llm-model",
                "qwen3:8b",
                "--llm-url",
                "http://localhost:1234/v1",
                "--llm-api-key-env",
                "TEST_API_KEY",
            ])),
            Ok(Command::Run(RunArgs {
                seed: 814_921,
                ticks: 288,
                database: None,
                resume: None,
                decision: DecisionArgs::OpenAi {
                    model: "qwen3:8b".into(),
                    base_url: "http://localhost:1234/v1".into(),
                    api_key_env: Some("TEST_API_KEY".into()),
                },
            }))
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
                resume: None,
                decision: DecisionArgs::Random,
            }))
        );
        assert_eq!(
            parse_args(args(&["run", "--resume", "run.sqlite", "--ticks", "10"])),
            Ok(Command::Run(RunArgs {
                seed: 814_921,
                ticks: 10,
                database: None,
                resume: Some(PathBuf::from("run.sqlite")),
                decision: DecisionArgs::Random,
            }))
        );
        assert_eq!(
            parse_args(args(&["run", "--days", "1", "--ticks", "2"])),
            Err(CliError::ConflictingDuration)
        );
        assert_eq!(
            parse_args(args(&["run", "--resume", "run.sqlite", "--seed", "1",])),
            Err(CliError::SeedWithResume)
        );
        assert_eq!(
            parse_args(args(&["run", "--llm-url", "http://localhost:1234/v1"])),
            Err(CliError::MissingLlmModel)
        );
        assert_eq!(
            parse_args(args(&["run", "--llm-api-key-env", "TEST_API_KEY"])),
            Err(CliError::MissingLlmModel)
        );
    }
}
