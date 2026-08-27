use terrarium::{
    decision::RandomDecisionEngine,
    observer::render_run,
    runner::run_simulation,
    sim::{Tick, World},
};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

#[derive(Debug, PartialEq, Eq)]
struct RunArgs {
    seed: u64,
    ticks: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
enum CliError {
    #[error("usage: terrarium run [--seed N] [--days N | --ticks N]")]
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

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<RunArgs, CliError> {
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("run") {
        return Err(CliError::Usage);
    }

    let mut seed = 814_921;
    let mut days = None;
    let mut ticks = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| CliError::MissingValue(flag.clone()))?;
        let number = value.parse::<u64>().map_err(|_| CliError::InvalidNumber {
            flag: flag.clone(),
            value,
        })?;
        match flag.as_str() {
            "--seed" => seed = number,
            "--days" => days = Some(number),
            "--ticks" => ticks = Some(number),
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
    Ok(RunArgs { seed, ticks })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .try_init()?;
    let args = parse_args(std::env::args().skip(1))?;
    let world = World::briar_glen(args.seed)?;
    let mut engine = RandomDecisionEngine::new(args.seed);
    let world = run_simulation(world, args.ticks, &mut engine).await?;
    println!("{}", render_run(&world));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CliError, RunArgs, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).into()).collect()
    }

    #[test]
    fn parses_run_options() {
        assert_eq!(
            parse_args(args(&["run", "--seed", "1234", "--days", "30"])),
            Ok(RunArgs {
                seed: 1_234,
                ticks: 8_640,
            })
        );
        assert_eq!(
            parse_args(args(&["run", "--ticks", "10000"])),
            Ok(RunArgs {
                seed: 814_921,
                ticks: 10_000,
            })
        );
    }

    #[test]
    fn rejects_conflicting_durations() {
        assert_eq!(
            parse_args(args(&["run", "--days", "1", "--ticks", "2"])),
            Err(CliError::ConflictingDuration)
        );
    }
}
