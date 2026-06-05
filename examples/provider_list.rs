//! Lists all builtin providers and their models from the embedded catalog.
//!
//! Run: `cargo run --example provider_list`

#[cfg(feature = "provider")]
fn main() {
    use llm_kernel::provider::ProviderIndex;

    let catalog = ProviderIndex::embedded();

    println!(
        "llm-kernel v{} — builtin providers\n",
        llm_kernel::version()
    );
    println!("{:<20} {:<25} {:<15} MODELS\n", "ID", "NAME", "CATEGORY");

    for id in catalog.ids() {
        let provider = catalog.get(&id).unwrap();
        let models = catalog.models_for(&id);
        println!(
            "{:<20} {:<25} {:<15} {}",
            id,
            provider.display_name,
            provider.category,
            models.len()
        );

        for model in models {
            let cost_info = model.cost.as_ref().map_or_else(
                || "  (no pricing)".to_string(),
                |c| format!("  ${:.2}/1M in, ${:.2}/1M out", c.input, c.output),
            );
            println!("  {:<18} {}", model.id, cost_info);
        }
    }
}

#[cfg(not(feature = "provider"))]
fn main() {
    println!("Enable the `provider` feature to run this example.");
}
