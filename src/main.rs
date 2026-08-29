use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufWriter, IsTerminal, Write},
    path::PathBuf,
    time::Duration,
};
use terrarium::{
    decision::{
        DEFAULT_LLM_CALLS_PER_DAY, HybridDecisionEngine, LocalDecisionEngine, OpenAiApi,
        OpenAiDecisionEngine, ReasoningEffort,
    },
    observer::{render_dashboard, render_event, render_run_since, render_summary},
    persistence::{load_world, save_world},
    runner::{LlmDecisionAudit, run_simulation, run_simulation_with_audit},
    sim::{AgentId, IntentionGoal, Tick, World},
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
    llm_log: Option<PathBuf>,
    decision: DecisionArgs,
}

#[derive(Debug, PartialEq)]
enum DecisionArgs {
    Local,
    OpenAi {
        model: String,
        base_url: String,
        api_key_env: Option<String>,
        api: OpenAiApi,
        temperature: f32,
        reasoning_effort: Option<ReasoningEffort>,
        max_completion_tokens: Option<u32>,
        provider: Option<String>,
        calls_per_day: u8,
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
        "usage: terrarium run [--seed N | --resume PATH] [--days N | --ticks N] [--database PATH] [--live] [--llm-model MODEL [--llm-url URL] [--llm-api chat|responses] [--llm-api-key-env NAME] [--llm-temperature 0..2] [--llm-reasoning-effort LEVEL] [--llm-max-tokens N] [--llm-provider PROVIDER] [--llm-calls-per-day N] [--llm-log PATH]]\n       terrarium inspect PATH"
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
    let mut llm_calls_per_day = None;
    let mut llm_log = None;
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
            "--llm-log" => llm_log = Some(value.into()),
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
            "--llm-calls-per-day" => {
                let calls = value.parse::<u8>().map_err(|_| CliError::InvalidLlmValue {
                    flag: flag.clone(),
                    value: value.clone(),
                })?;
                if calls == 0 {
                    return Err(CliError::InvalidLlmValue { flag, value });
                }
                llm_calls_per_day = Some(calls);
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
            calls_per_day: llm_calls_per_day.unwrap_or(DEFAULT_LLM_CALLS_PER_DAY),
        },
        None if llm_url.is_some()
            || llm_api_key_env.is_some()
            || llm_api.is_some()
            || llm_temperature.is_some()
            || llm_reasoning_effort.is_some()
            || llm_max_completion_tokens.is_some()
            || llm_provider.is_some()
            || llm_calls_per_day.is_some()
            || llm_log.is_some() =>
        {
            return Err(CliError::MissingLlmModel);
        }
        None => DecisionArgs::Local,
    };
    Ok(RunArgs {
        seed,
        ticks,
        database,
        resume,
        live,
        llm_log,
        decision,
    })
}

fn parse_number(flag: &str, value: String) -> Result<u64, CliError> {
    value.parse().map_err(|_| CliError::InvalidNumber {
        flag: flag.into(),
        value,
    })
}

#[derive(Debug, Error)]
enum AuditLogError {
    #[error("could not open LLM audit log {path}: {source}")]
    Open { path: PathBuf, source: io::Error },
    #[error("could not serialize LLM audit entry: {0}")]
    Serialize(serde_json::Error),
    #[error("could not write LLM audit log {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("could not flush LLM audit log {path}: {source}")]
    Flush { path: PathBuf, source: io::Error },
}

#[derive(Debug, Default, PartialEq, Eq)]
struct DialogueAudit {
    conversations: u64,
    partner_pairs: BTreeSet<(AgentId, AgentId)>,
    questions: u64,
}

impl DialogueAudit {
    fn record(&mut self, entry: &LlmDecisionAudit) {
        if entry.status == "started"
            && let IntentionGoal::Talk {
                target, message, ..
            } = &entry.intention
        {
            self.conversations += 1;
            self.partner_pairs.insert((entry.resident_id, *target));
            self.questions += u64::from(message.contains('?'));
        }
    }

    fn render(&self) -> Option<String> {
        (self.conversations > 0).then(|| {
            format!(
                "LLM dialogue: Talks {} | Partner pairs {} | Questions {} | Non-questions {}",
                self.conversations,
                self.partner_pairs.len(),
                self.questions,
                self.conversations - self.questions
            )
        })
    }
}

struct AuditLog {
    output: Option<(PathBuf, BufWriter<File>)>,
    error: Option<AuditLogError>,
    dialogue: DialogueAudit,
}

impl AuditLog {
    fn open(path: Option<PathBuf>) -> Result<Self, AuditLogError> {
        let output = path
            .map(|path| {
                File::create(&path)
                    .map(|file| (path.clone(), BufWriter::new(file)))
                    .map_err(|source| AuditLogError::Open { path, source })
            })
            .transpose()?;
        Ok(Self {
            output,
            error: None,
            dialogue: DialogueAudit::default(),
        })
    }

    fn record(&mut self, entry: &LlmDecisionAudit) {
        if self.error.is_some() {
            return;
        }
        self.dialogue.record(entry);
        let line = match serde_json::to_vec(entry) {
            Ok(line) => line,
            Err(error) => {
                self.error = Some(AuditLogError::Serialize(error));
                return;
            }
        };
        if let Some((path, output)) = &mut self.output
            && let Err(source) = output
                .write_all(&line)
                .and_then(|()| output.write_all(b"\n"))
        {
            self.error = Some(AuditLogError::Write {
                path: path.clone(),
                source,
            });
        }
    }

    fn finish(self) -> Result<DialogueAudit, AuditLogError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if let Some((path, mut output)) = self.output {
            output
                .flush()
                .map_err(|source| AuditLogError::Flush { path, source })?;
        }
        Ok(self.dialogue)
    }
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
    mut on_audit: impl FnMut(&LlmDecisionAudit),
) -> Result<World, Box<dyn std::error::Error + Send + Sync>> {
    let mut screen = LiveScreen::start()?;
    screen.draw(&world);
    let result = run_simulation_with_audit(
        world,
        ticks,
        engine,
        |world, _| screen.draw(world),
        |entry| on_audit(entry),
    )
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
                llm_log,
                decision,
            } = args;
            let world = match &resume {
                Some(path) => load_world(path)?,
                None => World::briar_glen(seed)?,
            };
            let world_seed = world.seed;
            let first_event = world.events().len();
            let (world, streamed, dialogue_summary) = match decision {
                DecisionArgs::Local => {
                    let mut engine = LocalDecisionEngine::new(world_seed);
                    if live {
                        (
                            run_live(world, ticks, &mut engine, |_| {}).await?,
                            false,
                            None,
                        )
                    } else {
                        (
                            run_simulation(world, ticks, &mut engine).await?,
                            false,
                            None,
                        )
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
                    calls_per_day,
                } => {
                    let mut llm = OpenAiDecisionEngine::new_with_api(
                        &base_url,
                        model,
                        Duration::from_secs(120),
                        api,
                    )?
                    .with_temperature(temperature)?;
                    if let Some(effort) = reasoning_effort {
                        llm = llm.with_reasoning_effort(effort);
                    }
                    if let Some(tokens) = max_completion_tokens {
                        llm = llm.with_max_completion_tokens(tokens)?;
                    }
                    if let Some(provider) = provider {
                        llm = llm.with_provider(provider)?;
                    }
                    if let Some(name) = api_key_env {
                        let key = std::env::var(&name)
                            .map_err(|_| CliError::MissingApiKey(name.clone()))?;
                        llm = llm
                            .with_api_key(key)
                            .map_err(|_| CliError::MissingApiKey(name))?;
                    }
                    let mut engine = HybridDecisionEngine::new(
                        LocalDecisionEngine::new(world_seed),
                        llm,
                        calls_per_day,
                    );
                    let mut audit = AuditLog::open(llm_log)?;
                    let result = if live {
                        (
                            run_live(world, ticks, &mut engine, |entry| audit.record(entry))
                                .await?,
                            false,
                        )
                    } else {
                        println!(
                            "{}\nSeed: {}\nAgents: {}\n",
                            world.name,
                            world.seed,
                            world.agents.len()
                        );
                        (
                            run_simulation_with_audit(
                                world,
                                ticks,
                                &mut engine,
                                |world, event| println!("{}", render_event(world, event)),
                                |entry| audit.record(entry),
                            )
                            .await?,
                            true,
                        )
                    };
                    let dialogue_summary = audit.finish()?.render();
                    (result.0, result.1, dialogue_summary)
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
            if let Some(summary) = dialogue_summary {
                println!("{summary}");
            }
        }
        Command::Inspect(path) => println!("{}", render_dashboard(&load_world(path)?)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AuditLog, AuditLogError, CliError, Command, DecisionArgs, RunArgs, parse_args};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    use terrarium::{
        decision::{OpenAiApi, ReasoningEffort},
        runner::LlmDecisionAudit,
        sim::{AgentId, DialogueTone, IntentionGoal, ProposedAction, Tick},
    };
    use uuid::Uuid;

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
                llm_log: None,
                decision: DecisionArgs::Local,
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
                "--llm-calls-per-day",
                "4",
                "--llm-log",
                "decisions.jsonl",
            ])),
            Ok(Command::Run(RunArgs {
                seed: 814_921,
                ticks: 288,
                database: None,
                resume: None,
                live: false,
                llm_log: Some(PathBuf::from("decisions.jsonl")),
                decision: DecisionArgs::OpenAi {
                    model: "qwen3:8b".into(),
                    base_url: "http://localhost:1234/v1".into(),
                    api_key_env: Some("TEST_API_KEY".into()),
                    api: OpenAiApi::Responses,
                    temperature: 0.7,
                    reasoning_effort: Some(ReasoningEffort::High),
                    max_completion_tokens: Some(512),
                    provider: Some("Anthropic".into()),
                    calls_per_day: 4,
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
                llm_log: None,
                decision: DecisionArgs::Local,
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
                llm_log: None,
                decision: DecisionArgs::Local,
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
            parse_args(args(&["run", "--llm-log", "decisions.jsonl"])),
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
        assert_eq!(
            parse_args(args(&[
                "run",
                "--llm-model",
                "model",
                "--llm-calls-per-day",
                "0"
            ])),
            Err(CliError::InvalidLlmValue {
                flag: "--llm-calls-per-day".into(),
                value: "0".into(),
            })
        );
    }

    #[test]
    fn dialogue_audit_reports_partner_and_question_diversity() {
        let speaker = AgentId(Uuid::nil());
        let mut log = AuditLog::open(None).expect("audit log");
        for (target, message) in [
            (AgentId(Uuid::max()), "Are you well?"),
            (AgentId(Uuid::from_u128(1)), "Thank you for helping."),
        ] {
            log.record(&LlmDecisionAudit {
                tick: Tick(1),
                resident_id: speaker,
                resident: "Alice Vale".into(),
                status: "started",
                proposal: None,
                intention: IntentionGoal::Talk {
                    target,
                    tone: DialogueTone::Friendly,
                    message: message.into(),
                },
                action: None,
                result: Some("success"),
                reason: None,
            });
        }

        assert_eq!(
            log.finish().expect("audit summary").render().as_deref(),
            Some("LLM dialogue: Talks 2 | Partner pairs 2 | Questions 1 | Non-questions 1")
        );
    }

    #[test]
    fn audit_log_writes_jsonl_and_reports_open_errors() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("terrarium-audit-{nonce}.jsonl"));
        let mut log = AuditLog::open(Some(path.clone())).expect("audit log");
        log.record(&LlmDecisionAudit {
            tick: Tick(1),
            resident_id: AgentId(Uuid::nil()),
            resident: "Alice Vale".into(),
            status: "started",
            proposal: Some(ProposedAction::Rest),
            intention: IntentionGoal::Rest,
            action: Some(ProposedAction::Rest),
            result: Some("success"),
            reason: None,
        });
        assert!(log.finish().expect("finish audit log").render().is_none());
        let lines = fs::read_to_string(&path).expect("audit file");
        assert_eq!(lines.lines().count(), 1);
        serde_json::from_str::<serde_json::Value>(lines.trim()).expect("valid JSONL");
        fs::remove_file(path).expect("remove audit file");

        let missing = std::env::temp_dir()
            .join(format!("terrarium-missing-{nonce}"))
            .join("audit.jsonl");
        assert!(matches!(
            AuditLog::open(Some(missing)),
            Err(AuditLogError::Open { .. })
        ));
    }
}
