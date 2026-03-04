//! Tag domain handlers.

use crate::state::AppState;

use super::{de, de_opt, ok_null, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "search_tags" => {
            let query: String = match de(args, "query") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let limit: Option<usize> = de_opt(args, "limit");
            let result =
                crate::tag_controller::TagController::search_tags(&state.db, query, limit).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        // PBI-038: Paged tag search for picker.
        "search_tags_paged" => {
            let query: String = de_opt(args, "query").unwrap_or_default();
            let limit: Option<usize> = de_opt(args, "limit");
            let offset: Option<usize> = de_opt(args, "offset");
            let result = crate::tag_controller::TagController::search_tags_paged(
                &state.db, query, limit, offset,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_all_tags_with_counts" => {
            let result =
                crate::tag_controller::TagController::get_all_tags_with_counts(&state.db).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_file_tags" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::tag_controller::TagController::get_entity_tags(&state.db, hash).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "add_tags" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let tag_strings: Vec<String> = match de(args, "tag_strings") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hash_clone = hash.clone();
            let result =
                crate::tag_controller::TagController::add_tags(&state.db, hash, tag_strings).await;
            match result {
                Ok(val) => {
                    if !val.is_empty() {
                        crate::events::emit_state_changed(
                            "add_tags",
                            crate::events::MutationImpact::file_tags(hash_clone)
                                .selection_summary(),
                        );
                    }
                    Some(to_json(&val))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "remove_tags" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let tag_strings: Vec<String> = match de(args, "tag_strings") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hash_clone = hash.clone();
            match crate::tag_controller::TagController::remove_tags(&state.db, hash, tag_strings)
                .await
            {
                Ok(()) => {
                    crate::events::emit_state_changed(
                        "remove_tags",
                        crate::events::MutationImpact::file_tags(hash_clone).selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "add_tags_batch" => {
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let tag_strings: Vec<String> = match de(args, "tag_strings") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes_clone = hashes.clone();
            match crate::tag_controller::TagController::add_tags_batch(
                &state.db,
                hashes,
                tag_strings,
            )
            .await
            {
                Ok(()) => {
                    if !hashes_clone.is_empty() {
                        crate::events::emit_state_changed(
                            "add_tags_batch",
                            crate::events::MutationImpact::batch_tags().file_hashes(hashes_clone),
                        );
                    }
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "remove_tags_batch" => {
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let tag_strings: Vec<String> = match de(args, "tag_strings") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes_clone = hashes.clone();
            match crate::tag_controller::TagController::remove_tags_batch(
                &state.db,
                hashes,
                tag_strings,
            )
            .await
            {
                Ok(()) => {
                    if !hashes_clone.is_empty() {
                        crate::events::emit_state_changed(
                            "remove_tags_batch",
                            crate::events::MutationImpact::batch_tags().file_hashes(hashes_clone),
                        );
                    }
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "find_files_by_tags" => {
            let tag_strings: Vec<String> = match de(args, "tag_strings") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let limit: Option<usize> = de_opt(args, "limit");
            let offset: Option<usize> = de_opt(args, "offset");
            let result = crate::tag_controller::TagController::find_files_by_tags(
                &state.db,
                tag_strings,
                limit,
                offset,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }

        // --- Tag aliases (siblings) ---
        "set_tag_alias" => {
            let from: String = match de(args, "from") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let to: String = match de(args, "to") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let (from_ns, from_st) = crate::sqlite::tags::parse_tag_string(&from);
            let (to_ns, to_st) = crate::sqlite::tags::parse_tag_string(&to);
            match state
                .db
                .add_sibling(&from_ns, &from_st, &to_ns, &to_st, "local")
                .await
            {
                Ok(()) => {
                    crate::events::emit_state_changed(
                        "set_tag_alias",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "remove_tag_alias" => {
            let from: String = match de(args, "from") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let (from_ns, from_st) = crate::sqlite::tags::parse_tag_string(&from);
            match state.db.remove_sibling(&from_ns, &from_st, "local").await {
                Ok(()) => {
                    crate::events::emit_state_changed(
                        "remove_tag_alias",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "get_tag_aliases" => {
            let result = state.db.with_conn(|conn| {
                let mut stmt = conn.prepare("SELECT t1.namespace || ':' || t1.subtag, t2.namespace || ':' || t2.subtag FROM tag_sibling ts JOIN tag t1 ON ts.from_tag_id = t1.tag_id JOIN tag t2 ON ts.to_tag_id = t2.tag_id")?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                let mut aliases: Vec<(String, String)> = Vec::new();
                for row in rows {
                    aliases.push(row?);
                }
                Ok(aliases)
            }).await;
            match result {
                Ok(aliases) => {
                    let json_aliases: Vec<serde_json::Value> = aliases
                        .iter()
                        .map(|(from, to)| serde_json::json!({"from": from, "to": to}))
                        .collect();
                    Some(to_json(&json_aliases))
                }
                Err(e) => Some(Err(e)),
            }
        }

        // --- Tag siblings/parents queries ---
        "get_tag_siblings_for_tag" => {
            let tag_id: i64 = match de(args, "tag_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.get_siblings_for_tag(tag_id).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_tag_parents_for_tag" => {
            let tag_id: i64 = match de(args, "tag_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.get_parents_for_tag(tag_id).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "add_tag_parent" => {
            let child: String = match de(args, "child") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let parent: String = match de(args, "parent") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let (cns, cst) = crate::sqlite::tags::parse_tag_string(&child);
            let (pns, pst) = crate::sqlite::tags::parse_tag_string(&parent);
            match state.db.add_parent(&cns, &cst, &pns, &pst, "local").await {
                Ok(()) => {
                    crate::events::emit_state_changed(
                        "add_tag_parent",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "remove_tag_parent" => {
            let child: String = match de(args, "child") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let parent: String = match de(args, "parent") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let (cns, cst) = crate::sqlite::tags::parse_tag_string(&child);
            let (pns, pst) = crate::sqlite::tags::parse_tag_string(&parent);
            match state
                .db
                .remove_parent(&cns, &cst, &pns, &pst, "local")
                .await
            {
                Ok(()) => {
                    crate::events::emit_state_changed(
                        "remove_tag_parent",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }

        // --- Merge tags ---
        "merge_tags" => {
            let from_tag: String = match de(args, "from_tag") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let to_tag: String = match de(args, "to_tag") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let (from_ns, from_st) = crate::sqlite::tags::parse_tag_string(&from_tag);
            let (to_ns, to_st) = crate::sqlite::tags::parse_tag_string(&to_tag);
            let result = state
                .db
                .with_conn(move |conn| {
                    let from_id = crate::sqlite::tags::get_or_create_tag(conn, &from_ns, &from_st)?;
                    let to_id = crate::sqlite::tags::get_or_create_tag(conn, &to_ns, &to_st)?;
                    // Collect affected file_ids before modifying
                    let mut stmt =
                        conn.prepare("SELECT entity_id FROM entity_tag_raw WHERE tag_id = ?1")?;
                    let file_ids: Vec<i64> = stmt
                        .query_map(rusqlite::params![from_id], |row| row.get(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    conn.execute(
                        "UPDATE OR IGNORE entity_tag_raw SET tag_id = ?1 WHERE tag_id = ?2",
                        rusqlite::params![to_id, from_id],
                    )?;
                    // Delete remaining (duplicates that already existed)
                    conn.execute(
                        "DELETE FROM entity_tag_raw WHERE tag_id = ?1",
                        rusqlite::params![from_id],
                    )?;
                    Ok((from_id, to_id, file_ids))
                })
                .await;
            match result {
                Ok((from_id, to_id, affected_file_ids)) => {
                    use crate::sqlite::compilers::CompilerEvent;
                    state
                        .db
                        .emit_compiler_event(CompilerEvent::TagChanged { tag_id: from_id });
                    state
                        .db
                        .emit_compiler_event(CompilerEvent::TagChanged { tag_id: to_id });
                    for file_id in affected_file_ids {
                        state
                            .db
                            .emit_compiler_event(CompilerEvent::FileTagsChanged { file_id });
                    }
                    crate::events::emit_state_changed(
                        "merge_tags",
                        crate::events::MutationImpact::new()
                            .domains(&[
                                crate::events::Domain::Tags,
                                crate::events::Domain::Sidebar,
                                crate::events::Domain::SmartFolders,
                            ])
                            .sidebar_tree()
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }

        // --- Tag type / namespace queries ---
        "lookup_tag_types" => {
            let result = state
                .db
                .with_read_conn(|conn| {
                    let mut stmt =
                        conn.prepare("SELECT DISTINCT namespace FROM tag WHERE file_count > 0")?;
                    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                    rows.collect::<rusqlite::Result<Vec<String>>>()
                })
                .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_tags_paginated" => {
            let namespace: Option<String> = de_opt(args, "namespace");
            let search: Option<String> = de_opt(args, "search");
            let cursor: Option<String> = de_opt(args, "cursor");
            let limit: i64 = de_opt(args, "limit").unwrap_or(200);
            let result = state
                .db
                .get_tags_paginated(namespace, search, cursor, limit)
                .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_namespace_summary" => {
            let result = state.db.get_namespace_summary().await;
            match result {
                Ok(data) => {
                    let json_result: Vec<serde_json::Value> = data
                        .iter()
                        .map(|(ns, count)| serde_json::json!({"namespace": ns, "count": count}))
                        .collect();
                    Some(to_json(&json_result))
                }
                Err(e) => Some(Err(e)),
            }
        }

        // --- Rename / delete tag ---
        "rename_tag" => {
            let tag_id: i64 = match de(args, "tag_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let new_name: String = match de(args, "new_name") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.rename_tag_by_id(tag_id, &new_name).await;
            match result {
                Ok((affected_file_ids, merged_into)) => {
                    crate::events::emit_state_changed(
                        "rename_tag",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(to_json(&serde_json::json!({
                        "affected_files": affected_file_ids.len(),
                        "merged_into": merged_into,
                    })))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_tag" => {
            let tag_id: i64 = match de(args, "tag_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.delete_tag_by_id(tag_id).await;
            match result {
                Ok(affected_file_ids) => {
                    crate::events::emit_state_changed(
                        "delete_tag",
                        crate::events::MutationImpact::new()
                            .domains(&[
                                crate::events::Domain::Tags,
                                crate::events::Domain::Sidebar,
                                crate::events::Domain::SmartFolders,
                            ])
                            .sidebar_tree()
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(to_json(&serde_json::json!({
                        "affected_files": affected_file_ids.len(),
                    })))
                }
                Err(e) => Some(Err(e)),
            }
        }

        // --- Normalize ingested namespaces ---
        "normalize_ingested_namespaces" => {
            let result = state.db.normalize_disallowed_namespaces().await;
            match result {
                Ok(stats) => {
                    if stats.tags_rewritten > 0 {
                        crate::events::emit_state_changed(
                            "normalize_ingested_namespaces",
                            crate::events::MutationImpact::new()
                                .domains(&[
                                    crate::events::Domain::Tags,
                                    crate::events::Domain::Sidebar,
                                    crate::events::Domain::SmartFolders,
                                ])
                                .sidebar_tree()
                                .grid_all()
                                .selection_summary(),
                        );
                    }
                    Some(to_json(&serde_json::json!({
                        "tags_rewritten": stats.tags_rewritten,
                        "tags_merged": stats.tags_merged,
                        "affected_files": stats.affected_files,
                    })))
                }
                Err(e) => Some(Err(e)),
            }
        }

        "companion_get_namespace_values" => {
            let namespace: String = match de(args, "namespace") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state
                .db
                .with_read_conn(move |conn| {
                    let mut stmt = conn.prepare(
                        "SELECT subtag, file_count FROM tag
                     WHERE namespace = ?1 AND file_count > 0
                     ORDER BY file_count DESC",
                    )?;
                    let rows = stmt.query_map([&namespace], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                    })?;
                    let mut values = Vec::new();
                    for row in rows {
                        let (subtag, count) = row?;
                        values.push(serde_json::json!({
                            "value": subtag,
                            "count": count,
                            "thumbnail_hash": null,
                        }));
                    }
                    Ok(values)
                })
                .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "companion_get_files_by_tag" => {
            let tag: String = match de(args, "tag") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::tag_controller::TagController::find_files_by_tags(
                &state.db,
                vec![tag],
                None,
                None,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }

        _ => None,
    }
}
