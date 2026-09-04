//! Explicit support command: close Picto first, then pass library root and hashes.
use std::sync::Arc;

fn main() -> Result<(), String> {
    // Debug builds of the application dispatcher exceed Windows' small default
    // executable stack. Keep this support process bounded to one worker.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?
                .block_on(repair_selected())
        })
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "Repair worker panicked".to_string())?
}

async fn repair_selected() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .ok_or("Usage: repair_vp9 LIBRARY_ROOT HASH...")?;
    let hashes = args.collect::<Vec<_>>();
    if hashes.is_empty() {
        return Err("Select at least one affected hash".into());
    }
    let database_path = std::path::Path::new(&root).join("library.sqlite");
    let backup = std::path::Path::new(&root).join(format!(
        "pre-vp9-repair-{}.sqlite",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    let database = rusqlite::Connection::open_with_flags(
        &database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    database
        .backup("main", &backup, None)
        .map_err(|e| e.to_string())?;
    drop(database);
    println!("Database backup: {}", backup.display());
    let app = Arc::new(picto_core::library_application::LibraryApplication::open(
        &root,
    )?);
    for hash in hashes {
        println!("Repairing {hash}");
        let result = picto_core::ipc::dispatch_library_async(
            &app,
            "media.repair_vp9",
            &serde_json::json!({"file_hash":hash}).to_string(),
        )
        .await?;
        println!("{}", result.ok_or("Repair command unavailable")?);
    }
    Ok(())
}
