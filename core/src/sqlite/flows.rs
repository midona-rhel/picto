//! Flow CRUD — parent entity that groups subscriptions with a shared schedule.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub flow_id: i64,
    pub name: String,
    pub schedule: String,
    pub created_at: String,
}

pub fn create_flow(conn: &Connection, name: &str) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO flow (name, schedule, created_at) VALUES (?1, 'manual', ?2)",
        params![name, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_flow(conn: &Connection, flow_id: i64) -> rusqlite::Result<Option<Flow>> {
    conn.query_row(
        "SELECT flow_id, name, schedule, created_at FROM flow WHERE flow_id = ?1",
        [flow_id],
        |row| {
            Ok(Flow {
                flow_id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                created_at: row.get(3)?,
            })
        },
    )
    .optional()
}

pub fn list_flows(conn: &Connection) -> rusqlite::Result<Vec<Flow>> {
    let mut stmt =
        conn.prepare_cached("SELECT flow_id, name, schedule, created_at FROM flow ORDER BY name")?;
    let rows = stmt.query_map([], |row| {
        Ok(Flow {
            flow_id: row.get(0)?,
            name: row.get(1)?,
            schedule: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn delete_flow(conn: &Connection, flow_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM flow WHERE flow_id = ?1", [flow_id])?;
    Ok(())
}

pub fn rename_flow(conn: &Connection, flow_id: i64, name: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE flow SET name = ?1 WHERE flow_id = ?2",
        params![name, flow_id],
    )?;
    Ok(())
}

pub fn set_flow_schedule(conn: &Connection, flow_id: i64, schedule: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE flow SET schedule = ?1 WHERE flow_id = ?2",
        params![schedule, flow_id],
    )?;
    Ok(())
}

pub fn get_flow_subscription_ids(conn: &Connection, flow_id: i64) -> rusqlite::Result<Vec<i64>> {
    let mut stmt =
        conn.prepare_cached("SELECT subscription_id FROM subscription WHERE flow_id = ?1")?;
    let rows = stmt.query_map([flow_id], |row| row.get(0))?;
    rows.collect()
}

pub fn get_flow_total_file_count(conn: &Connection, flow_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM subscription_entity
         WHERE subscription_id IN (SELECT subscription_id FROM subscription WHERE flow_id = ?1)",
        [flow_id],
        |row| row.get(0),
    )
}

impl SqliteDatabase {
    pub async fn create_flow(&self, name: &str) -> Result<Flow, String> {
        let n = name.to_string();
        let flow_id = self.with_conn(move |conn| create_flow(conn, &n)).await?;
        let fid = flow_id;
        self.with_read_conn(move |conn| get_flow(conn, fid))
            .await?
            .ok_or_else(|| "Flow not found after creation".to_string())
    }

    pub async fn list_flows(&self) -> Result<Vec<Flow>, String> {
        self.with_read_conn(list_flows).await
    }

    pub async fn get_flow(&self, flow_id: i64) -> Result<Option<Flow>, String> {
        self.with_read_conn(move |conn| get_flow(conn, flow_id))
            .await
    }

    pub async fn delete_flow(&self, flow_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| delete_flow(conn, flow_id)).await
    }

    pub async fn rename_flow(&self, flow_id: i64, name: &str) -> Result<(), String> {
        let n = name.to_string();
        self.with_conn(move |conn| rename_flow(conn, flow_id, &n))
            .await
    }

    pub async fn set_flow_schedule(&self, flow_id: i64, schedule: &str) -> Result<(), String> {
        let s = schedule.to_string();
        self.with_conn(move |conn| set_flow_schedule(conn, flow_id, &s))
            .await
    }

    pub async fn get_flow_subscription_ids(&self, flow_id: i64) -> Result<Vec<i64>, String> {
        self.with_read_conn(move |conn| get_flow_subscription_ids(conn, flow_id))
            .await
    }

    pub async fn get_flow_total_file_count(&self, flow_id: i64) -> Result<i64, String> {
        self.with_read_conn(move |conn| get_flow_total_file_count(conn, flow_id))
            .await
    }

    pub async fn list_subscriptions_for_flow(
        &self,
        flow_id: i64,
    ) -> Result<Vec<super::subscriptions::Subscription>, String> {
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT subscription_id, name, site_id, paused, flow_id,
                        initial_file_limit, periodic_file_limit, created_at
                 FROM subscription WHERE flow_id = ?1 ORDER BY name",
            )?;
            let rows = stmt.query_map([flow_id], |row| {
                Ok(super::subscriptions::Subscription {
                    subscription_id: row.get(0)?,
                    name: row.get(1)?,
                    site_id: row.get(2)?,
                    paused: row.get::<_, i64>(3)? != 0,
                    flow_id: row.get(4)?,
                    initial_file_limit: row.get(5)?,
                    periodic_file_limit: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?;
            rows.collect()
        })
        .await
    }
}
