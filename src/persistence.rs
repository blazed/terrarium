use crate::sim::{Agent, Event, Location, Tick, TownEvent, World, WorldError};
use rusqlite::{Connection, OpenFlags, params};
use serde::de::DeserializeOwned;
use std::{num::ParseIntError, path::Path};
use thiserror::Error;

const CHECKPOINT_VERSION: i64 = 11;

#[derive(Debug, Clone, PartialEq)]
pub struct StoredRun {
    pub name: String,
    pub seed: u64,
    pub tick: Tick,
    pub agents: Vec<Agent>,
    pub locations: Vec<Location>,
    pub active_town_event: Option<TownEvent>,
    pub events: Vec<Event>,
}

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    InvalidWorld(#[from] WorldError),
    #[error("invalid persisted {field} value {value:?}: {source}")]
    InvalidNumber {
        field: &'static str,
        value: String,
        source: ParseIntError,
    },
    #[error("too many events to persist")]
    TooManyEvents,
    #[error("unsupported checkpoint version {0}")]
    UnsupportedCheckpointVersion(i64),
}

pub fn save_world(path: impl AsRef<Path>, world: &World) -> Result<(), PersistenceError> {
    world.validate()?;
    let agents = world
        .agents
        .values()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let locations = world
        .locations
        .values()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let events = world
        .events()
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?;

    let mut connection = Connection::open(path)?;
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA user_version = 11;
         CREATE TABLE IF NOT EXISTS world (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             name TEXT NOT NULL,
             seed TEXT NOT NULL,
             tick TEXT NOT NULL,
             active_town_event TEXT NOT NULL CHECK (json_valid(active_town_event))
         );
         CREATE TABLE IF NOT EXISTS agents (
             id TEXT PRIMARY KEY,
             json TEXT NOT NULL CHECK (json_valid(json))
         );
         CREATE TABLE IF NOT EXISTS locations (
             id TEXT PRIMARY KEY,
             json TEXT NOT NULL CHECK (json_valid(json))
         );
         CREATE TABLE IF NOT EXISTS events (
             sequence INTEGER PRIMARY KEY CHECK (sequence >= 0),
             id TEXT NOT NULL UNIQUE,
             tick TEXT NOT NULL,
             json TEXT NOT NULL CHECK (json_valid(json))
         );
         DELETE FROM events;
         DELETE FROM agents;
         DELETE FROM locations;
         DELETE FROM world;",
    )?;
    transaction.execute(
        "INSERT INTO world (id, name, seed, tick, active_town_event) VALUES (1, ?1, ?2, ?3, ?4)",
        params![
            world.name,
            world.seed.to_string(),
            world.tick.0.to_string(),
            serde_json::to_string(&world.active_town_event)?
        ],
    )?;
    {
        let mut statement = transaction.prepare("INSERT INTO agents (id, json) VALUES (?1, ?2)")?;
        for (agent, json) in world.agents.values().zip(&agents) {
            statement.execute(params![agent.id.to_string(), json])?;
        }
    }
    {
        let mut statement =
            transaction.prepare("INSERT INTO locations (id, json) VALUES (?1, ?2)")?;
        for (location, json) in world.locations.values().zip(&locations) {
            statement.execute(params![location.id.to_string(), json])?;
        }
    }
    {
        let mut statement = transaction
            .prepare("INSERT INTO events (sequence, id, tick, json) VALUES (?1, ?2, ?3, ?4)")?;
        for (sequence, (event, json)) in world.events().iter().zip(&events).enumerate() {
            let sequence = i64::try_from(sequence).map_err(|_| PersistenceError::TooManyEvents)?;
            statement.execute(params![
                sequence,
                event.id.to_string(),
                event.tick.0.to_string(),
                json
            ])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn load_run(path: impl AsRef<Path>) -> Result<StoredRun, PersistenceError> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let (name, seed, tick, active_town_event): (String, String, String, String) = connection
        .query_row(
            "SELECT name, seed, tick, active_town_event FROM world WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;

    Ok(StoredRun {
        name,
        seed: parse_number("seed", seed)?,
        tick: Tick(parse_number("tick", tick)?),
        agents: load_json_rows(&connection, "SELECT json FROM agents ORDER BY id")?,
        locations: load_json_rows(&connection, "SELECT json FROM locations ORDER BY id")?,
        active_town_event: serde_json::from_str(&active_town_event)?,
        events: load_json_rows(&connection, "SELECT json FROM events ORDER BY sequence")?,
    })
}

pub fn load_world(path: impl AsRef<Path>) -> Result<World, PersistenceError> {
    let path = path.as_ref();
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version != CHECKPOINT_VERSION {
        return Err(PersistenceError::UnsupportedCheckpointVersion(version));
    }
    drop(connection);

    let run = load_run(path)?;
    Ok(World::from_snapshot(
        run.name,
        run.seed,
        run.tick,
        run.agents,
        run.locations,
        run.active_town_event,
        run.events,
    )?)
}

fn parse_number(field: &'static str, value: String) -> Result<u64, PersistenceError> {
    value
        .parse()
        .map_err(|source| PersistenceError::InvalidNumber {
            field,
            value,
            source,
        })
}

fn load_json_rows<T: DeserializeOwned>(
    connection: &Connection,
    query: &str,
) -> Result<Vec<T>, PersistenceError> {
    let mut statement = connection.prepare(query)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}

#[cfg(test)]
mod tests {
    use super::{PersistenceError, load_run, load_world, save_world};
    use crate::{
        decision::LocalDecisionEngine,
        runner::run_simulation,
        sim::{ActionResult, Intention, IntentionGoal, Item, ProposedAction, Tick, World},
    };
    use rusqlite::Connection;
    use std::{env, fs, path::PathBuf};

    fn test_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!("terrarium-{}-{name}.sqlite", std::process::id()))
    }

    #[tokio::test]
    async fn completed_world_round_trips_through_sqlite() {
        let path = test_path("round-trip");
        let _ = fs::remove_file(&path);
        let world = World::briar_glen(u64::MAX).expect("town");
        let mut engine = LocalDecisionEngine::new(u64::MAX);
        let mut world = run_simulation(world, 50, &mut engine)
            .await
            .expect("simulation");
        let residents = world.agents.keys().copied().take(2).collect::<Vec<_>>();
        let actor = residents[0];
        let receiver = residents[1];
        let location = world.agents[&actor].location;
        world.relocate(receiver, location);
        world
            .agents
            .get_mut(&actor)
            .expect("resident")
            .inventory
            .meals = 1;
        world
            .agents
            .get_mut(&receiver)
            .expect("resident")
            .needs
            .food = 0.1;
        assert!(matches!(
            world.execute(
                actor,
                ProposedAction::Give {
                    target: receiver,
                    item: Item::Meal,
                },
            ),
            ActionResult::Success(_)
        ));
        let actor = world.agents.get_mut(&actor).expect("resident");
        actor.intention = Some(Intention {
            goal: IntentionGoal::Rest,
            expires_at: Tick(world.tick.0 + 10),
        });
        actor.health = 0.73;
        actor.injury = true;
        actor.disease = crate::sim::DiseaseState::Incubating {
            until: Tick(world.tick.0 + 10),
        };

        save_world(&path, &world).expect("save");
        let stored = load_run(&path).expect("load");

        assert_eq!(stored.name, world.name);
        assert_eq!(stored.seed, world.seed);
        assert_eq!(stored.tick, world.tick);
        assert_eq!(
            stored.agents,
            world.agents.values().cloned().collect::<Vec<_>>()
        );
        assert_eq!(
            stored.locations,
            world.locations.values().cloned().collect::<Vec<_>>()
        );
        assert_eq!(stored.active_town_event, world.active_town_event);
        assert_eq!(stored.events, world.events());
        assert!(stored.agents.iter().any(|agent| !agent.memories.is_empty()));
        assert!(
            stored
                .locations
                .iter()
                .any(|location| location.business.is_some())
        );
        assert_eq!(load_world(&path).expect("checkpoint"), world);
        fs::remove_file(path).expect("cleanup");
    }

    #[tokio::test]
    async fn split_run_matches_an_uninterrupted_run() {
        let path = test_path("resume");
        let _ = fs::remove_file(&path);
        let seed = 12_345;

        let mut continuous_engine = LocalDecisionEngine::new(seed);
        let continuous = run_simulation(
            World::briar_glen(seed).expect("town"),
            900,
            &mut continuous_engine,
        )
        .await
        .expect("continuous run");

        let mut first_engine = LocalDecisionEngine::new(seed);
        let first = run_simulation(
            World::briar_glen(seed).expect("town"),
            400,
            &mut first_engine,
        )
        .await
        .expect("first run");
        save_world(&path, &first).expect("checkpoint");
        let resumed = load_world(&path).expect("load checkpoint");
        let mut resumed_engine = LocalDecisionEngine::new(resumed.seed);
        let resumed = run_simulation(resumed, 500, &mut resumed_engine)
            .await
            .expect("resumed run");

        assert_eq!(resumed, continuous);
        fs::remove_file(path).expect("cleanup");
    }

    #[tokio::test]
    async fn incompatible_and_corrupt_checkpoints_are_rejected() {
        let path = test_path("invalid");
        let _ = fs::remove_file(&path);
        let seed = 77;
        let mut engine = LocalDecisionEngine::new(seed);
        let world = run_simulation(World::briar_glen(seed).expect("town"), 1, &mut engine)
            .await
            .expect("run");
        save_world(&path, &world).expect("checkpoint");

        let connection = Connection::open(&path).expect("database");
        connection
            .execute_batch("PRAGMA user_version = 12")
            .expect("version");
        assert!(matches!(
            load_world(&path),
            Err(PersistenceError::UnsupportedCheckpointVersion(12))
        ));

        connection
            .execute_batch("PRAGMA user_version = 6")
            .expect("old version");
        assert!(matches!(
            load_world(&path),
            Err(PersistenceError::UnsupportedCheckpointVersion(6))
        ));

        connection
            .execute_batch("PRAGMA user_version = 11; UPDATE world SET tick = '0'")
            .expect("corrupt checkpoint");
        assert!(matches!(
            load_world(&path),
            Err(PersistenceError::InvalidWorld(_))
        ));
        drop(connection);
        fs::remove_file(path).expect("cleanup");
    }
}
