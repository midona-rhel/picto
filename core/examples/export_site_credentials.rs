//! One-shot export of every stored site credential to a verifier credential
//! file, so a full certification campaign asks the OS keychain exactly once
//! instead of prompting per site and per rebuilt test binary.
//!
//! Usage: cargo run --example export_site_credentials -- <output.json>
//! The output feeds `verify-sites.mjs --credential-file`. It contains live
//! secrets: write it to a temporary location and delete it after the runs.

use std::collections::BTreeMap;

fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: export_site_credentials <output.json>")?;
    let mut credentials = BTreeMap::new();
    let mut missing = Vec::new();
    for site in picto_core::subscriptions::sites::SITES {
        let owner = site.credential_owner_site_id;
        if credentials.contains_key(owner) {
            continue;
        }
        match picto_core::credential_store::get_credential(owner) {
            Ok(Some(credential)) => {
                credentials.insert(owner.to_string(), credential);
            }
            Ok(None) => missing.push(owner),
            Err(error) => eprintln!("{owner}: {error}"),
        }
    }
    let file = serde_json::json!({ "credentials": credentials });
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    println!(
        "exported {} credentials to {path}; no stored credential for: {}",
        credentials.len(),
        missing.join(", ")
    );
    Ok(())
}
