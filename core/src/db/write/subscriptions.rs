//! Subscription write operations — groups, subscriptions, queries.

use rusqlite::{params, Connection};

pub fn create_group(conn: &Connection, name: &str, now: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO subscription_group (name, date_added) VALUES (?1, ?2)",
        params![name, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_group(conn: &Connection, group_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM subscription_group WHERE group_id = ?1", [group_id])?;
    Ok(())
}

pub fn rename_group(conn: &Connection, group_id: i64, name: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE subscription_group SET name = ?1 WHERE group_id = ?2", params![name, group_id])?;
    Ok(())
}

pub fn set_group_schedule(conn: &Connection, group_id: i64, schedule: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE subscription_group SET schedule = ?1 WHERE group_id = ?2", params![schedule, group_id])?;
    Ok(())
}

pub fn create_subscription(
    conn: &Connection,
    name: &str,
    site_id: &str,
    group_id: Option<i64>,
    initial_post_limit: Option<i64>,
    periodic_post_limit: Option<i64>,
    now: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO subscription (name, site_id, group_id, initial_post_limit, periodic_post_limit, date_added)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![name, site_id, group_id, initial_post_limit.unwrap_or(100), periodic_post_limit.unwrap_or(100), now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_subscription(conn: &Connection, subscription_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM subscription WHERE subscription_id = ?1", [subscription_id])?;
    Ok(())
}

pub fn rename_subscription(conn: &Connection, subscription_id: i64, name: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE subscription SET name = ?1 WHERE subscription_id = ?2", params![name, subscription_id])?;
    Ok(())
}

pub fn pause_subscription(conn: &Connection, subscription_id: i64, paused: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE subscription SET paused = ?1 WHERE subscription_id = ?2", params![paused as i64, subscription_id])?;
    Ok(())
}

pub fn add_query(conn: &Connection, subscription_id: i64, query_text: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO subscription_query (subscription_id, query_text) VALUES (?1, ?2)",
        params![subscription_id, query_text],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_query(conn: &Connection, query_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM subscription_query WHERE query_id = ?1", [query_id])?;
    Ok(())
}

pub fn edit_query(conn: &Connection, query_id: i64, query_text: &str, display_name: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE subscription_query SET query_text = ?1, display_name = ?2 WHERE query_id = ?3",
        params![query_text, display_name, query_id],
    )?;
    Ok(())
}

pub fn pause_query(conn: &Connection, query_id: i64, paused: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE subscription_query SET paused = ?1 WHERE query_id = ?2", params![paused as i64, query_id])?;
    Ok(())
}
