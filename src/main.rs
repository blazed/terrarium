use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    time::Duration,
};
use terrarium::{
    decision::{OpenAiApi, OpenAiDecisionEngine, RandomDecisionEngine, ReasoningEffort},
    observer::{render_dashboard, render_event, render_run_since, render_summary},
    persistence::{load_world, save_world},
    runner::{run_simulation, run_simulation_with_events},
    sim::{Tick, World},
};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

#[derive(Debug, PartialEq)]
struct RunArgs {
    seed: u64,
    ticks: u64,
    database: Option<PathBuf>,
    resume: Option<PathBuf>,
    live: bool,
    decision: DecisionArgs,
}

#[derive(Debug, PartialEq)]
enum DecisionArgs {
    Random,
    OpenAi {
        model: String,
        base_url: String,
        api_key_env: Option<String>,
        api: OpenAiApi,
        temperature: f32,
        reasoning_effort: Option<ReasoningEffort>,
        max_completion_tokens: Option<u32>,
        provider: Option<String>,
    },
}

#[derive(Debug, PartialEq)]
enum Command {
    Run(RunArgs),
    Inspect(PathBuf),
}

#[derive(Debug, Error, PartialEq, Eq)]
enum CliError {
    #[error(
        "usage: terrarium run [--seed N | --resume PATH] [--days N | --ticks N] [--database PATH] [--live] [--llm-model MODEL [--llm-url URL] [--llm-api chat|responses] [--llm-api-key-env NAME] [--llm-temperature 0..2] [--llm-reasoning-effort LEVEL] [--llm-max-tokens N] [--llm-provider PROVIDER]]\n       terrarium inspect PATH"
    )]
    Usage,
    #[error("missing value for {0}")]
    MissingValue(String),
    #[error("invalid value for {flag}: {value}")]
    InvalidNumber { flag: String, value: String },
    #[error("invalid LLM value for {flag}: {value}")]
    InvalidLlmValue { flag: String, value: String },
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
    let mut llm_api = None;
    let mut llm_temperature = None;
    let mut llm_reasoning_effort = None;
    let mut llm_max_completion_tokens = None;
    let mut llm_provider = None;
    let mut live = false;
    while let Some(flag) = args.next() {
        if flag == "--live" {
            live = true;
            continue;
        }
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
            "--llm-api" => {
                llm_api = Some(value.parse().map_err(|_| CliError::InvalidLlmValue {
                    flag: flag.clone(),
                    value: value.clone(),
                })?);
            }
            "--llm-temperature" => {
                let temperature = value
                    .parse::<f32>()
                    .map_err(|_| CliError::InvalidLlmValue {
                        flag: flag.clone(),
                        value: value.clone(),
                    })?;
                if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
                    return Err(CliError::InvalidLlmValue { flag, value });
                }
                llm_temperature = Some(temperature);
            }
            "--llm-reasoning-effort" => {
                llm_reasoning_effort =
                    Some(value.parse().map_err(|_| CliError::InvalidLlmValue {
                        flag: flag.clone(),
                        value: value.clone(),
                    })?);
            }
            "--llm-max-tokens" => {
                let tokens = value
                    .parse::<u32>()
                    .map_err(|_| CliError::InvalidLlmValue {
                        flag: flag.clone(),
                        value: value.clone(),
                    })?;
                if tokens == 0 {
                    return Err(CliError::InvalidLlmValue { flag, value });
                }
                llm_max_completion_tokens = Some(tokens);
            }
            "--llm-provider" => {
                if value.trim().is_empty() {
                    return Err(CliError::InvalidLlmValue { flag, value });
                }
                llm_provider = Some(value.trim().into());
            }
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
            api: llm_api.unwrap_or(OpenAiApi::ChatCompletions),
            temperature: llm_temperature.unwrap_or(0.0),
            reasoning_effort: llm_reasoning_effort,
            max_completion_tokens: llm_max_completion_tokens,
            provider: llm_provider,
        },
        None if llm_url.is_some()
            || llm_api_key_env.is_some()
            || llm_api.is_some()
            || llm_temperature.is_some()
            || llm_reasoning_effort.is_some()
            || llm_max_completion_tokens.is_some()
            || llm_provider.is_some() =>
        {
            return Err(CliError::MissingLlmModel);
        }
        None => DecisionArgs::Random,
    };
    Ok(RunArgs {
        seed,
        ticks,
        database,
        resume,
        live,
        decision,
    })
}

fn parse_number(flag: &str, value: String) -> Result<u64, CliError> {
    value.parse().map_err(|_| CliError::InvalidNumber {
        flag: flag.into(),
        value,
    })
}

struct LiveScreen {
    terminal: bool,
}

impl LiveScreen {
    fn start() -> io::Result<Self> {
        let mut screen = Self {
            terminal: io::stdout().is_terminal(),
        };
        if screen.terminal {
            screen.write("\x1b[?1049h\x1b[?25l")?;
        }
        Ok(screen)
    }

    fn draw(&mut self, world: &World) {
        if self.terminal {
            let _ = self.write(&format!("\x1b[H\x1b[2J{}", render_dashboard(world)));
        }
    }

    fn write(&mut self, text: &str) -> io::Result<()> {
        let mut output = io::stdout().lock();
        output.write_all(text.as_bytes())?;
        output.flush()
    }
}

impl Drop for LiveScreen {
    fn drop(&mut self) {
        if self.terminal {
            let _ = self.write("\x1b[?25h\x1b[?1049l");
        }
    }
}

async fn run_live(
    world: World,
    ticks: u64,
    engine: &mut impl terrarium::decision::DecisionEngine,
) -> Result<World, Box<dyn std::error::Error + Send + Sync>> {
    let mut screen = LiveScreen::start()?;
    screen.draw(&world);
    let result = run_simulation_with_events(world, ticks, engine, |world, _| {
        screen.draw(world);
    })
    .await;
    Ok(result?)
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
                live,
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
                    if live {
                        (run_live(world, ticks, &mut engine).await?, false)
                    } else {
                        (run_simulation(world, ticks, &mut engine).await?, false)
                    }
                }
                DecisionArgs::OpenAi {
                    model,
                    base_url,
                    api_key_env,
                    api,
                    temperature,
                    reasoning_effort,
                    max_completion_tokens,
                    provider,
                } => {
                    let mut engine = OpenAiDecisionEngine::new_with_api(
                        &base_url,
                        model,
                        Duration::from_secs(120),
                        api,
                    )?
                    .with_temperature(temperature)?;
                    if let Some(effort) = reasoning_effort {
                        engine = engine.with_reasoning_effort(effort);
                    }
                    if let Some(tokens) = max_completion_tokens {
                        engine = engine.with_max_completion_tokens(tokens)?;
                    }
                    if let Some(provider) = provider {
                        engine = engine.with_provider(provider)?;
                    }
                    if let Some(name) = api_key_env {
                        let key = std::env::var(&name)
                            .map_err(|_| CliError::MissingApiKey(name.clone()))?;
                        engine = engine
                            .with_api_key(key)
                            .map_err(|_| CliError::MissingApiKey(name))?;
                    }
                    if live {
                        (run_live(world, ticks, &mut engine).await?, false)
                    } else {
                        println!(
                            "{}\nSeed: {}\nAgents: {}\n",
                            world.name,
                            world.seed,
                            world.agents.len()
                        );
                        (
                            run_simulation_with_events(
                                world,
                                ticks,
                                &mut engine,
                                |world, event| {
                                    println!("{}", render_event(world, event));
                                },
                            )
                            .await?,
                            true,
                        )
                    }
                }
            };
            if let Some(path) = database.or(resume) {
                save_world(path, &world)?;
            }
            if live {
                println!("{}", render_dashboard(&world));
            } else if streamed {
                println!("\n{}", render_summary(&world));
            } else {
                println!("{}", render_run_since(&world, first_event));
            }
        }
        Command::Inspect(path) => println!("{}", render_dashboard(&load_world(path)?)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CliError, Command, DecisionArgs, RunArgs, parse_args};
    use std::path::PathBuf;
    use terrarium::decision::{OpenAiApi, ReasoningEffort};

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
                "--live",
            ])),
            Ok(Command::Run(RunArgs {
                seed: 1_234,
                ticks: 8_640,
                database: Some(PathBuf::from("run.sqlite")),
                resume: None,
                live: true,
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
                "--llm-api",
                "responses",
                "--llm-temperature",
                "0.7",
                "--llm-reasoning-effort",
                "high",
                "--llm-max-tokens",
                "512",
                "--llm-provider",
                "Anthropic",
            ])),
            Ok(Command::Run(RunArgs {
                seed: 814_921,
                ticks: 288,
                database: None,
                resume: None,
                live: false,
                decision: DecisionArgs::OpenAi {
                    model: "qwen3:8b".into(),
                    base_url: "http://localhost:1234/v1".into(),
                    api_key_env: Some("TEST_API_KEY".into()),
                    api: OpenAiApi::Responses,
                    temperature: 0.7,
                    reasoning_effort: Some(ReasoningEffort::High),
                    max_completion_tokens: Some(512),
                    provider: Some("Anthropic".into()),
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
                live: false,
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
                live: false,
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
        assert_eq!(
            parse_args(args(&["run", "--llm-api", "responses"])),
            Err(CliError::MissingLlmModel)
        );
        assert_eq!(
            parse_args(args(&[
                "run",
                "--llm-model",
                "model",
                "--llm-api",
                "unknown"
            ])),
            Err(CliError::InvalidLlmValue {
                flag: "--llm-api".into(),
                value: "unknown".into(),
            })
        );
        assert_eq!(
            parse_args(args(&[
                "run",
                "--llm-model",
                "model",
                "--llm-temperature",
                "3"
            ])),
            Err(CliError::InvalidLlmValue {
                flag: "--llm-temperature".into(),
                value: "3".into(),
            })
        );
        assert_eq!(
            parse_args(args(&[
                "run",
                "--llm-model",
                "model",
                "--llm-reasoning-effort",
                "extreme"
            ])),
            Err(CliError::InvalidLlmValue {
                flag: "--llm-reasoning-effort".into(),
                value: "extreme".into(),
            })
        );
        assert_eq!(
            parse_args(args(&[
                "run",
                "--llm-model",
                "model",
                "--llm-max-tokens",
                "0"
            ])),
            Err(CliError::InvalidLlmValue {
                flag: "--llm-max-tokens".into(),
                value: "0".into(),
            })
        );
        assert_eq!(
            parse_args(args(&["run", "--llm-model", "model", "--llm-provider", ""])),
            Err(CliError::InvalidLlmValue {
                flag: "--llm-provider".into(),
                value: "".into(),
            })
        );
    }
}
