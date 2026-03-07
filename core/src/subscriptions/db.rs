//! Subscription + query + file + credential-domain CRUD.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::sqlite::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub subscription_id: i64,
    pub name: String,
    pub site_id: String,
    pub paused: bool,
    pub flow_id: Option<i64>,
    pub initial_file_limit: i64,
    pub periodic_file_limit: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionQuery {
    pub query_id: i64,
    pub subscription_id: i64,
    pub query_text: String,
    pub display_name: Option<String>,
    pub paused: bool,
    pub last_check_time: Option<String>,
    pub files_found: i64,
    pub completed_initial_run: bool,
    pub resume_cursor: Option<String>,
    pub resume_strategy: Option<String>,
}

fn map_subscription_row(row: &rusqlite::Row) -> rusqlite::Result<Subscription> {
    Ok(Subscription {
        subscription_id: row.get(0)?,
        name: row.get(1)?,
        site_id: row.get(2)?,
        paused: row.get::<_, i64>(3)? != 0,
        flow_id: row.get(4)?,
        initial_file_limit: row.get(5)?,
        periodic_file_limit: row.get(6)?,
        created_at: row.get(7)?,
    })
}

const SUB_COLS: &str = "subscription_id, name, site_id, paused, flow_id, initial_file_limit, periodic_file_limit, created_at";

fn map_query_row(row: &rusqlite::Row) -> rusqlite::Result<SubscriptionQuery> {
    Ok(SubscriptionQuery {
        query_id: row.get(0)?,
        subscription_id: row.get(1)?,
        query_text: row.get(2)?,
        display_name: row.get(3)?,
        paused: row.get::<_, i64>(4)? != 0,
        last_check_time: row.get(5)?,
        files_found: row.get(6)?,
        completed_initial_run: row.get::<_, i64>(7)? != 0,
        resume_cursor: row.get(8)?,
        resume_strategy: row.get(9)?,
    })
}

const QUERY_COLS: &str = "query_id, subscription_id, query_text, display_name, paused, last_check_time, files_found, completed_initial_run, resume_cursor, resume_strategy";

pub fn create_subscription(
    conn: &Connection,
    name: &str,
    site_id: &str,
    flow_id: Option<i64>,
) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut stmt = conn.prepare("PRAGMA table_info(subscription)")?;
    let mut has_site_id = false;
    let mut has_site_plugin_id = false;
    let col_rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for col_name in col_rows {
        let col_name = col_name?;
        if col_name == "site_id" {
            has_site_id = true;
        } else if col_name == "site_plugin_id" {
            has_site_plugin_id = true;
        }
    }

    if has_site_id && has_site_plugin_id {
        conn.execute(
            "INSERT INTO subscription (name, site_id, site_plugin_id, flow_id, created_at)
             VALUES (?1, ?2, ?2, ?3, ?4)",
            params![name, site_id, flow_id, now],
        )?;
    } else if has_site_id {
        conn.execute(
            "INSERT INTO subscription (name, site_id, flow_id, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![name, site_id, flow_id, now],
        )?;
    } else if has_site_plugin_id {
        conn.execute(
            "INSERT INTO subscription (name, site_plugin_id, flow_id, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![name, site_id, flow_id, now],
        )?;
    } else {
        return Err(rusqlite::Error::InvalidColumnName(
            "subscription.site_id/site_plugin_id".to_string(),
        ));
    }

    Ok(conn.last_insert_rowid())
}

pub fn get_subscription(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Option<Subscription>> {
    conn.query_row(
        &format!("SELECT {SUB_COLS} FROM subscription WHERE subscription_id = ?1"),
        [subscription_id],
        map_subscription_row,
    )
    .optional()
}

pub fn list_subscriptions(conn: &Connection) -> rusqlite::Result<Vec<Subscription>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {SUB_COLS} FROM subscription ORDER BY name"
    ))?;
    let rows = stmt.query_map([], map_subscription_row)?;
    rows.collect()
}

pub fn delete_subscription(conn: &Connection, subscription_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM subscription WHERE subscription_id = ?1",
        [subscription_id],
    )?;
    Ok(())
}

pub fn set_subscription_paused(
    conn: &Connection,
    subscription_id: i64,
    paused: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription SET paused = ?1 WHERE subscription_id = ?2",
        params![paused as i64, subscription_id],
    )?;
    Ok(())
}

pub fn add_subscription_query(
    conn: &Connection,
    subscription_id: i64,
    query_text: &str,
    display_name: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO subscription_query (subscription_id, query_text, display_name) VALUES (?1, ?2, ?3)",
        params![subscription_id, query_text, display_name],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_subscription_queries(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Vec<SubscriptionQuery>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {QUERY_COLS} FROM subscription_query WHERE subscription_id = ?1"
    ))?;
    let rows = stmt.query_map([subscription_id], map_query_row)?;
    rows.collect()
}

pub fn get_subscription_query(
    conn: &Connection,
    query_id: i64,
) -> rusqlite::Result<Option<SubscriptionQuery>> {
    conn.query_row(
        &format!("SELECT {QUERY_COLS} FROM subscription_query WHERE query_id = ?1"),
        [query_id],
        map_query_row,
    )
    .optional()
}

pub fn delete_subscription_query(conn: &Connection, query_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM subscription_query WHERE query_id = ?1",
        [query_id],
    )?;
    Ok(())
}

pub fn set_query_paused(conn: &Connection, query_id: i64, paused: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query SET paused = ?1 WHERE query_id = ?2",
        params![paused as i64, query_id],
    )?;
    Ok(())
}

pub fn update_query_progress(
    conn: &Connection,
    query_id: i64,
    last_check_time: &str,
    files_found: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query SET last_check_time = ?1, files_found = ?2
         WHERE query_id = ?3",
        params![last_check_time, files_found, query_id],
    )?;
    Ok(())
}

pub fn reset_query_progress(conn: &Connection, query_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query
         SET files_found = 0,
             last_check_time = NULL,
             completed_initial_run = 0,
             resume_cursor = NULL,
             resume_strategy = NULL
         WHERE query_id = ?1",
        [query_id],
    )?;
    Ok(())
}

pub fn reset_subscription_state(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<(usize, usize, usize)> {
    let tx = conn.unchecked_transaction()?;

    let queries_reset = tx.execute(
        "UPDATE subscription_query
         SET files_found = 0,
             last_check_time = NULL,
             completed_initial_run = 0,
             resume_cursor = NULL,
             resume_strategy = NULL
         WHERE subscription_id = ?1",
        [subscription_id],
    )?;

    let entities_deleted = tx.execute(
        "DELETE FROM subscription_entity WHERE subscription_id = ?1",
        [subscription_id],
    )?;

    let post_maps_deleted = tx.execute(
        "DELETE FROM subscription_post_collection WHERE subscription_id = ?1",
        [subscription_id],
    )?;

    tx.commit()?;
    Ok((queries_reset, entities_deleted, post_maps_deleted))
}

pub fn set_query_resume_state(
    conn: &Connection,
    query_id: i64,
    resume_cursor: Option<&str>,
    resume_strategy: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query
         SET resume_cursor = ?1, resume_strategy = ?2
         WHERE query_id = ?3",
        params![resume_cursor, resume_strategy, query_id],
    )?;
    Ok(())
}

pub fn set_query_completed_initial_run(
    conn: &Connection,
    query_id: i64,
    completed: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query SET completed_initial_run = ?1 WHERE query_id = ?2",
        params![completed as i64, query_id],
    )?;
    Ok(())
}

pub fn add_subscription_entity(
    conn: &Connection,
    subscription_id: i64,
    entity_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO subscription_entity (subscription_id, entity_id) VALUES (?1, ?2)",
        params![subscription_id, entity_id],
    )?;
    Ok(())
}

pub fn upsert_subscription_post_collection(
    conn: &Connection,
    subscription_id: i64,
    site_id: &str,
    post_id: &str,
    collection_entity_id: i64,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subscription_post_collection (
             subscription_id, site_id, post_id, collection_entity_id, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(subscription_id, site_id, post_id)
         DO UPDATE SET collection_entity_id = excluded.collection_entity_id,
                       updated_at = excluded.updated_at",
        params![subscription_id, site_id, post_id, collection_entity_id, now],
    )?;
    Ok(())
}

pub fn get_subscription_post_collection(
    conn: &Connection,
    subscription_id: i64,
    site_id: &str,
    post_id: &str,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT collection_entity_id
         FROM subscription_post_collection
         WHERE subscription_id = ?1 AND site_id = ?2 AND post_id = ?3",
        params![subscription_id, site_id, post_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_subscription_entity_ids(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn
        .prepare_cached("SELECT entity_id FROM subscription_entity WHERE subscription_id = ?1")?;
    let rows = stmt.query_map([subscription_id], |row| row.get(0))?;
    rows.collect()
}

pub fn rename_subscription(
    conn: &Connection,
    subscription_id: i64,
    name: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription SET name = ?1 WHERE subscription_id = ?2",
        params![name, subscription_id],
    )?;
    Ok(())
}

pub fn get_subscription_entity_count(
    conn: &Connection,
    subscription_id: i64,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM subscription_entity WHERE subscription_id = ?1",
        [subscription_id],
        |row| row.get(0),
    )
}

/// All subscriptions with aggregated file counts in a single query.
pub fn list_subscriptions_with_file_counts(
    conn: &Connection,
) -> rusqlite::Result<Vec<(Subscription, i64)>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT s.{}, COALESCE(fc.cnt, 0)
             FROM subscription s
             LEFT JOIN (
                 SELECT subscription_id, COUNT(*) AS cnt
                 FROM subscription_entity GROUP BY subscription_id
             ) fc ON fc.subscription_id = s.subscription_id
             ORDER BY s.name",
        SUB_COLS.replace(", ", ", s.")
    ))?;
    let rows = stmt.query_map([], |row| {
        Ok((map_subscription_row(row)?, row.get::<_, i64>(8)?))
    })?;
    rows.collect()
}

/// All subscription queries in a single query (no filter — caller groups by subscription_id).
pub fn list_all_subscription_queries(
    conn: &Connection,
) -> rusqlite::Result<Vec<SubscriptionQuery>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {QUERY_COLS} FROM subscription_query ORDER BY subscription_id, query_id"
    ))?;
    let rows = stmt.query_map([], map_query_row)?;
    rows.collect()
}

/// Subscriptions for a given flow with aggregated file counts — single query.
pub fn list_subscriptions_for_flow_with_file_counts(
    conn: &Connection,
    flow_id: i64,
) -> rusqlite::Result<Vec<(Subscription, i64)>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT s.{}, COALESCE(fc.cnt, 0)
             FROM subscription s
             LEFT JOIN (
                 SELECT subscription_id, COUNT(*) AS cnt
                 FROM subscription_entity GROUP BY subscription_id
             ) fc ON fc.subscription_id = s.subscription_id
             WHERE s.flow_id = ?1
             ORDER BY s.name",
        SUB_COLS.replace(", ", ", s.")
    ))?;
    let rows = stmt.query_map([flow_id], |row| {
        Ok((map_subscription_row(row)?, row.get::<_, i64>(8)?))
    })?;
    rows.collect()
}

/// All subscription queries belonging to a flow — single query.
pub fn list_subscription_queries_for_flow(
    conn: &Connection,
    flow_id: i64,
) -> rusqlite::Result<Vec<SubscriptionQuery>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT sq.{}
             FROM subscription_query sq
             INNER JOIN subscription s ON s.subscription_id = sq.subscription_id
             WHERE s.flow_id = ?1
             ORDER BY sq.subscription_id, sq.query_id",
        QUERY_COLS.replace(", ", ", sq.")
    ))?;
    let rows = stmt.query_map([flow_id], map_query_row)?;
    rows.collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialDomain {
    pub site_category: String,
    pub credential_type: String,
    pub display_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialHealth {
    pub site_category: String,
    pub health_status: String,
    pub last_checked_at: String,
    pub last_error: Option<String>,
}

pub fn upsert_credential_domain(
    conn: &Connection,
    site_category: &str,
    credential_type: &str,
    display_name: Option<&str>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO credential_domain (site_category, credential_type, display_name, created_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(site_category) DO UPDATE SET credential_type = ?2, display_name = ?3",
        params![site_category, credential_type, display_name, now],
    )?;
    Ok(())
}

pub fn list_credential_domains(conn: &Connection) -> rusqlite::Result<Vec<CredentialDomain>> {
    let mut stmt = conn.prepare_cached(
        "SELECT site_category, credential_type, display_name, created_at
         FROM credential_domain ORDER BY site_category",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CredentialDomain {
            site_category: row.get(0)?,
            credential_type: row.get(1)?,
            display_name: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn delete_credential_domain(conn: &Connection, site_category: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM credential_domain WHERE site_category = ?1",
        [site_category],
    )?;
    Ok(())
}

pub fn upsert_credential_health(
    conn: &Connection,
    site_category: &str,
    health_status: &str,
    last_error: Option<&str>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO credential_health (site_category, health_status, last_checked_at, last_error)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(site_category)
         DO UPDATE SET health_status = excluded.health_status,
                       last_checked_at = excluded.last_checked_at,
                       last_error = excluded.last_error",
        params![site_category, health_status, now, last_error],
    )?;
    Ok(())
}

pub fn list_credential_health(conn: &Connection) -> rusqlite::Result<Vec<CredentialHealth>> {
    let mut stmt = conn.prepare_cached(
        "SELECT site_category, health_status, last_checked_at, last_error
         FROM credential_health
         ORDER BY site_category",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CredentialHealth {
            site_category: row.get(0)?,
            health_status: row.get(1)?,
            last_checked_at: row.get(2)?,
            last_error: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn delete_credential_health(conn: &Connection, site_category: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM credential_health WHERE site_category = ?1",
        [site_category],
    )?;
    Ok(())
}

impl SqliteDatabase {
    pub async fn create_subscription(
        &self,
        name: &str,
        site_id: &str,
        flow_id: Option<i64>,
    ) -> Result<Subscription, String> {
        let n = name.to_string();
        let tmpl = site_id.to_string();
        let sub_id = self
            .with_conn(move |conn| create_subscription(conn, &n, &tmpl, flow_id))
            .await?;
        let sid = sub_id;
        self.with_read_conn(move |conn| get_subscription(conn, sid))
            .await?
            .ok_or_else(|| "Subscription not found after creation".to_string())
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<Subscription>, String> {
        self.with_read_conn(list_subscriptions).await
    }

    pub async fn delete_subscription(&self, subscription_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| delete_subscription(conn, subscription_id))
            .await
    }

    pub async fn set_subscription_paused(
        &self,
        subscription_id: i64,
        paused: bool,
    ) -> Result<(), String> {
        self.with_conn(move |conn| set_subscription_paused(conn, subscription_id, paused))
            .await
    }

    pub async fn add_subscription_query(
        &self,
        subscription_id: i64,
        query_text: &str,
        display_name: Option<&str>,
    ) -> Result<SubscriptionQuery, String> {
        let qt = query_text.to_string();
        let dn = display_name.map(|s| s.to_string());
        let qid = self
            .with_conn(move |conn| {
                add_subscription_query(conn, subscription_id, &qt, dn.as_deref())
            })
            .await?;
        Ok(SubscriptionQuery {
            query_id: qid,
            subscription_id,
            query_text: query_text.to_string(),
            display_name: display_name.map(|s| s.to_string()),
            paused: false,
            last_check_time: None,
            files_found: 0,
            completed_initial_run: false,
            resume_cursor: None,
            resume_strategy: None,
        })
    }

    pub async fn get_subscription_queries(
        &self,
        subscription_id: i64,
    ) -> Result<Vec<SubscriptionQuery>, String> {
        self.with_read_conn(move |conn| get_subscription_queries(conn, subscription_id))
            .await
    }

    pub async fn delete_subscription_query(&self, query_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| delete_subscription_query(conn, query_id))
            .await
    }

    pub async fn set_query_paused(&self, query_id: i64, paused: bool) -> Result<(), String> {
        self.with_conn(move |conn| set_query_paused(conn, query_id, paused))
            .await
    }

    pub async fn update_query_progress(
        &self,
        query_id: i64,
        last_check_time: &str,
        files_found: i64,
    ) -> Result<(), String> {
        let lct = last_check_time.to_string();
        self.with_conn(move |conn| update_query_progress(conn, query_id, &lct, files_found))
            .await
    }

    pub async fn reset_query_progress(&self, query_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| reset_query_progress(conn, query_id))
            .await
    }

    pub async fn reset_subscription_state(
        &self,
        subscription_id: i64,
    ) -> Result<(usize, usize, usize), String> {
        self.with_conn(move |conn| reset_subscription_state(conn, subscription_id))
            .await
    }

    pub async fn set_query_completed_initial_run(
        &self,
        query_id: i64,
        completed: bool,
    ) -> Result<(), String> {
        self.with_conn(move |conn| set_query_completed_initial_run(conn, query_id, completed))
            .await
    }

    pub async fn set_query_resume_state(
        &self,
        query_id: i64,
        resume_cursor: Option<String>,
        resume_strategy: Option<String>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| {
            set_query_resume_state(
                conn,
                query_id,
                resume_cursor.as_deref(),
                resume_strategy.as_deref(),
            )
        })
        .await
    }

    pub async fn add_subscription_entity(
        &self,
        subscription_id: i64,
        hash: &str,
    ) -> Result<(), String> {
        let entity_id = self.resolve_hash(hash).await?;
        self.with_conn(move |conn| add_subscription_entity(conn, subscription_id, entity_id))
            .await
    }

    pub async fn upsert_subscription_post_collection(
        &self,
        subscription_id: i64,
        site_id: &str,
        post_id: &str,
        collection_entity_id: i64,
    ) -> Result<(), String> {
        let site = site_id.to_string();
        let post = post_id.to_string();
        self.with_conn(move |conn| {
            upsert_subscription_post_collection(
                conn,
                subscription_id,
                &site,
                &post,
                collection_entity_id,
            )
        })
        .await
    }

    pub async fn get_subscription_post_collection(
        &self,
        subscription_id: i64,
        site_id: &str,
        post_id: &str,
    ) -> Result<Option<i64>, String> {
        let site = site_id.to_string();
        let post = post_id.to_string();
        self.with_read_conn(move |conn| {
            get_subscription_post_collection(conn, subscription_id, &site, &post)
        })
        .await
    }

    pub async fn rename_subscription(
        &self,
        subscription_id: i64,
        name: &str,
    ) -> Result<(), String> {
        let n = name.to_string();
        self.with_conn(move |conn| rename_subscription(conn, subscription_id, &n))
            .await
    }

    pub async fn get_subscription_entity_count(&self, subscription_id: i64) -> Result<i64, String> {
        self.with_read_conn(move |conn| get_subscription_entity_count(conn, subscription_id))
            .await
    }

    pub async fn list_subscriptions_with_file_counts(
        &self,
    ) -> Result<Vec<(Subscription, i64)>, String> {
        self.with_read_conn(list_subscriptions_with_file_counts)
            .await
    }

    pub async fn list_all_subscription_queries(
        &self,
    ) -> Result<Vec<SubscriptionQuery>, String> {
        self.with_read_conn(list_all_subscription_queries).await
    }

    pub async fn list_subscriptions_for_flow_with_file_counts(
        &self,
        flow_id: i64,
    ) -> Result<Vec<(Subscription, i64)>, String> {
        self.with_read_conn(move |conn| list_subscriptions_for_flow_with_file_counts(conn, flow_id))
            .await
    }

    pub async fn list_subscription_queries_for_flow(
        &self,
        flow_id: i64,
    ) -> Result<Vec<SubscriptionQuery>, String> {
        self.with_read_conn(move |conn| list_subscription_queries_for_flow(conn, flow_id))
            .await
    }

    pub async fn upsert_credential_domain(
        &self,
        site_category: &str,
        credential_type: &str,
        display_name: Option<&str>,
    ) -> Result<(), String> {
        let sc = site_category.to_string();
        let ct = credential_type.to_string();
        let dn = display_name.map(|s| s.to_string());
        self.with_conn(move |conn| upsert_credential_domain(conn, &sc, &ct, dn.as_deref()))
            .await
    }

    pub async fn list_credential_domains(&self) -> Result<Vec<CredentialDomain>, String> {
        self.with_read_conn(list_credential_domains).await
    }

    pub async fn delete_credential_domain(&self, site_category: &str) -> Result<(), String> {
        let sc = site_category.to_string();
        self.with_conn(move |conn| delete_credential_domain(conn, &sc))
            .await
    }

    pub async fn upsert_credential_health(
        &self,
        site_category: &str,
        health_status: &str,
        last_error: Option<&str>,
    ) -> Result<(), String> {
        let sc = site_category.to_string();
        let hs = health_status.to_string();
        let le = last_error.map(|s| s.to_string());
        self.with_conn(move |conn| upsert_credential_health(conn, &sc, &hs, le.as_deref()))
            .await
    }

    pub async fn list_credential_health(&self) -> Result<Vec<CredentialHealth>, String> {
        self.with_read_conn(list_credential_health).await
    }

    pub async fn delete_credential_health(&self, site_category: &str) -> Result<(), String> {
        let sc = site_category.to_string();
        self.with_conn(move |conn| delete_credential_health(conn, &sc))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::create_subscription;
    use rusqlite::Connection;

    #[test]
    fn create_subscription_handles_legacy_site_plugin_id_not_null() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE subscription (
                 subscription_id         INTEGER PRIMARY KEY,
                 name                    TEXT NOT NULL,
                 site_id                 TEXT NOT NULL,
                 site_plugin_id          TEXT NOT NULL,
                 paused                  INTEGER NOT NULL DEFAULT 0,
                 flow_id                 INTEGER,
                 initial_file_limit      INTEGER NOT NULL DEFAULT 100,
                 periodic_file_limit     INTEGER NOT NULL DEFAULT 50,
                 created_at              TEXT NOT NULL
             );",
        )
        .expect("create subscription table");

        let sub_id = create_subscription(&conn, "Test", "e621", None)
            .expect("insert subscription with dual site columns");

        let (site_id, site_plugin_id): (String, String) = conn
            .query_row(
                "SELECT site_id, site_plugin_id FROM subscription WHERE subscription_id = ?1",
                [sub_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read inserted subscription");

        assert_eq!(site_id, "e621");
        assert_eq!(site_plugin_id, "e621");
    }
}
