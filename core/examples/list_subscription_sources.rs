use picto_core::subscriptions::sites::SITES;
use picto_core::subscriptions::source_adapter::describe_site;

fn main() {
    let sources = SITES
        .iter()
        .map(|site| {
            serde_json::json!({
                "site": site,
                "adapter": describe_site(site.id),
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::to_string_pretty(&sources).expect("serialize subscription source registry")
    );
}
