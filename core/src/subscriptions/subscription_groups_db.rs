//! Subscription group CRUD — parent entity that groups subscriptions with a shared schedule.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::sqlite::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionGroup {
    pub group_id: i64,
    pub name: String,
    pub schedule: String,
    pub created_at: String,
}

pub fn create_group(conn: &Connection, name: &str) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_group (name, schedule, created_at) VALUES (?1, 'manual', ?2)",
        params![name, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_group(conn: &Connection, group_id: i64) -> rusqlite::Result<Option<SubscriptionGroup>> {
    conn.query_row(
        "SELECT group_id, name, schedule, created_at FROM subscription_group WHERE group_id = ?1",
        [group_id],
        |row| {
            Ok(SubscriptionGroup {
                group_id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                created_at: row.get(3)?,
            })
        },
    )
    .optional()
}

pub fn list_groups(conn: &Connection) -> rusqlite::Result<Vec<SubscriptionGroup>> {
    let mut stmt = conn.prepare_cached(
        "SELECT group_id, name, schedule, created_at FROM subscription_group ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SubscriptionGroup {
            group_id: row.get(0)?,
            name: row.get(1)?,
            schedule: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn delete_group(conn: &Connection, group_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM subscription_group WHERE group_id = ?1",
        [group_id],
    )?;
    Ok(())
}

pub fn rename_group(conn: &Connection, group_id: i64, name: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_group SET name = ?1 WHERE group_id = ?2",
        params![name, group_id],
    )?;
    Ok(())
}

pub fn set_group_schedule(
    conn: &Connection,
    group_id: i64,
    schedule: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_group SET schedule = ?1 WHERE group_id = ?2",
        params![schedule, group_id],
    )?;
    Ok(())
}

pub fn get_group_subscription_ids(conn: &Connection, group_id: i64) -> rusqlite::Result<Vec<i64>> {
    let mut stmt =
        conn.prepare_cached("SELECT subscription_id FROM subscription WHERE group_id = ?1")?;
    let rows = stmt.query_map([group_id], |row| row.get(0))?;
    rows.collect()
}

impl SqliteDatabase {
    pub async fn create_group(&self, name: &str) -> Result<SubscriptionGroup, String> {
        let n = name.to_string();
        let group_id = self.with_conn(move |conn| create_group(conn, &n)).await?;
        let fid = group_id;
        self.with_read_conn(move |conn| get_group(conn, fid))
            .await?
            .ok_or_else(|| "Group not found after creation".to_string())
    }

    pub async fn list_groups(&self) -> Result<Vec<SubscriptionGroup>, String> {
        self.with_read_conn(list_groups).await
    }

    pub async fn get_group(&self, group_id: i64) -> Result<Option<SubscriptionGroup>, String> {
        self.with_read_conn(move |conn| get_group(conn, group_id))
            .await
    }

    pub async fn delete_group(&self, group_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| delete_group(conn, group_id))
            .await
    }

    pub async fn rename_group(&self, group_id: i64, name: &str) -> Result<(), String> {
        let n = name.to_string();
        self.with_conn(move |conn| rename_group(conn, group_id, &n))
            .await
    }

    pub async fn set_group_schedule(&self, group_id: i64, schedule: &str) -> Result<(), String> {
        let s = schedule.to_string();
        self.with_conn(move |conn| set_group_schedule(conn, group_id, &s))
            .await
    }

    pub async fn get_group_subscription_ids(&self, group_id: i64) -> Result<Vec<i64>, String> {
        self.with_read_conn(move |conn| get_group_subscription_ids(conn, group_id))
            .await
    }

    pub async fn list_subscriptions_for_group(
        &self,
        group_id: i64,
    ) -> Result<Vec<super::db::Subscription>, String> {
        self.with_read_conn(move |conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT subscription_id, name, site_id, paused, group_id,
                        initial_post_limit, periodic_post_limit, auto_collections, created_at
                 FROM subscription WHERE group_id = ?1 ORDER BY name",
            )?;
            let rows = stmt.query_map([group_id], |row| {
                Ok(super::db::Subscription {
                    subscription_id: row.get(0)?,
                    name: row.get(1)?,
                    site_id: row.get(2)?,
                    paused: row.get::<_, i64>(3)? != 0,
                    group_id: row.get(4)?,
                    initial_post_limit: row.get(5)?,
                    periodic_post_limit: row.get(6)?,
                    auto_collections: row.get::<_, i64>(7)? != 0,
                    created_at: row.get(8)?,
                })
            })?;
            rows.collect()
        })
        .await
    }
}
