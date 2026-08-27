use crate::sim::{Agent, Event, Location, Tick, World, WorldError};
use rusqlite::{Connection, OpenFlags, params};
use serde::de::DeserializeOwned;
use std::{num::ParseIntError, path::Path};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct StoredRun {
    pub name: String,
    pub seed: u64,
    pub tick: Tick,
    pub agents: Vec<Agent>,
    pub locations: Vec<Location>,
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
         CREATE TABLE IF NOT EXISTS world (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             name TEXT NOT NULL,
             seed TEXT NOT NULL,
             tick TEXT NOT NULL
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
        "INSERT INTO world (id, name, seed, tick) VALUES (1, ?1, ?2, ?3)",
        params![world.name, world.seed.to_string(), world.tick.0.to_string()],
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
    let (name, seed, tick): (String, String, String) = connection.query_row(
        "SELECT name, seed, tick FROM world WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    Ok(StoredRun {
        name,
        seed: parse_number("seed", seed)?,
        tick: Tick(parse_number("tick", tick)?),
        agents: load_json_rows(&connection, "SELECT json FROM agents ORDER BY id")?,
        locations: load_json_rows(&connection, "SELECT json FROM locations ORDER BY id")?,
        events: load_json_rows(&connection, "SELECT json FROM events ORDER BY sequence")?,
    })
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
    use super::{load_run, save_world};
    use crate::{decision::RandomDecisionEngine, runner::run_simulation, sim::World};
    use std::{env, fs};

    #[tokio::test]
    async fn completed_world_round_trips_through_sqlite() {
        let path = env::temp_dir().join(format!("terrarium-{}.sqlite", std::process::id()));
        let _ = fs::remove_file(&path);
        let world = World::briar_glen(u64::MAX).expect("town");
        let mut engine = RandomDecisionEngine::new(u64::MAX);
        let world = run_simulation(world, 50, &mut engine)
            .await
            .expect("simulation");

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
        assert_eq!(stored.events, world.events());
        fs::remove_file(path).expect("cleanup");
    }
}
