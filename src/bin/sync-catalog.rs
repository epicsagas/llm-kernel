//! Catalog sync CLI — refresh `src/provider/catalog.json` from models.dev.
//!
//! The merge engine lives in [`llm_kernel::provider::sync`]. This binary is a
//! thin wrapper: read the current catalog, fetch models.dev, merge, and either
//! report drift (`--check`) or write the result atomically.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run --bin llm-kernel-sync-catalog --features catalog-sync -- --check
//! cargo run --bin llm-kernel-sync-catalog --features catalog-sync
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use llm_kernel::discovery::ModelsDevPayload;
use llm_kernel::provider::sync;

/// Refresh the provider catalog from the models.dev upstream.
#[derive(Parser, Debug)]
#[command(name = "llm-kernel-sync-catalog", version, about)]
struct Cli {
    /// Path to catalog.json.
    #[arg(long, default_value = "src/provider/catalog.json")]
    catalog: PathBuf,
    /// Detect drift only: print the diff and exit non-zero if the catalog is
    /// stale, without writing.
    #[arg(long)]
    check: bool,
    /// Override the models.dev API URL.
    ///
    /// Trust boundary: forwarded verbatim to the fetcher — pass only
    /// admin-configured values, never untrusted input.
    #[arg(long)]
    api_url: Option<String>,
}

/// Apply a fetched upstream payload to the catalog file.
///
/// Split out of [`run`] so the file I/O + check/write flow is testable without
/// network access.
fn apply(catalog: &Path, upstream: ModelsDevPayload, check: bool) -> anyhow::Result<ExitCode> {
    let current_json = std::fs::read_to_string(catalog)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", catalog.display()))?;
    let current = sync::parse_catalog(&current_json)?;
    let (merged, diff) = sync::merge_catalog(&current, &upstream)?;

    print!("{diff}");

    if check {
        if merged == current {
            println!("catalog is in sync with models.dev");
            return Ok(ExitCode::SUCCESS);
        }
        println!("catalog is stale (drift detected)");
        return Ok(ExitCode::from(1));
    }

    // Atomic write: stage to a sibling temp file, then rename.
    let merged_json = sync::serialize_catalog(&merged)?;
    let tmp = catalog.with_file_name(format!(
        "{}.tmp",
        catalog
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "catalog.json".to_string())
    ));
    std::fs::write(&tmp, merged_json)
        .map_err(|e| anyhow::anyhow!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, catalog)
        .map_err(|e| anyhow::anyhow!("renaming {} -> {}: {e}", tmp.display(), catalog.display()))?;
    println!("wrote {}", catalog.display());
    Ok(ExitCode::SUCCESS)
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let upstream = sync::fetch_models_dev(cli.api_url.as_deref())?;
    apply(&cli.catalog, upstream, cli.check)
}

fn main() -> anyhow::Result<ExitCode> {
    run(Cli::parse())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A minimal zai catalog with a deliberately stale glm-5 price.
    const STALE_CATALOG: &str = r#"{
  "providers": [
    {
      "id": "zai", "display_name": "Z.AI", "description": "", "category": "international",
      "family": "zai", "auth_mode": "secret", "key_var": "ZAI_API_KEY",
      "literal_auth_token": "", "base_url": "https://z.ai", "default_model": "glm-5",
      "model_tiers": {}, "model_choices": [], "test_url": "https://z.ai",
      "setup": [], "usage": [], "api_base_url": null, "npm_package": null, "doc_url": null,
      "models": [
        {"id": "glm-5", "name": "GLM-5 (stale)", "family": null, "release_date": null,
         "cost": {"input": 0.5, "output": 0.5, "cache_read": null, "cache_write": null},
         "limit": {"context": 128000, "output": 4096}, "modalities": null,
         "capabilities": {"attachment": false, "reasoning": false, "temperature": true, "tool_call": true, "streaming": true},
         "knowledge": null}
      ]
    }
  ]
}
"#;

    const UPSTREAM: &str = r#"{
    "zai": {
      "id": "zai", "env": ["ZHIPU_API_KEY"], "api": "https://api.z.ai/api/paas/v4",
      "npm": "@ai-sdk/zai", "doc": "https://docs.z.ai",
      "models": {
        "glm-5": {
          "id": "glm-5", "name": "GLM-5", "family": "glm",
          "reasoning": true, "tool_call": true, "temperature": true,
          "limit": {"context": 204800, "output": 131072},
          "cost": {"input": 1, "output": 3.2, "cache_read": 0.2, "cache_write": 0}
        }
      }
    }
}"#;

    fn parse_upstream() -> ModelsDevPayload {
        serde_json::from_str(UPSTREAM).unwrap()
    }

    #[test]
    fn test_check_detects_drift() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("catalog.json");
        fs::write(&catalog, STALE_CATALOG).unwrap();
        let code = apply(&catalog, parse_upstream(), true).unwrap();
        assert_eq!(code, ExitCode::from(1));
        // --check must not mutate the file.
        assert_eq!(fs::read_to_string(&catalog).unwrap(), STALE_CATALOG);
    }

    #[test]
    fn test_write_updates_stale_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("catalog.json");
        fs::write(&catalog, STALE_CATALOG).unwrap();

        let code = apply(&catalog, parse_upstream(), false).unwrap();
        assert_eq!(code, ExitCode::SUCCESS);

        let after = fs::read_to_string(&catalog).unwrap();
        let providers = sync::parse_catalog(&after).unwrap();
        let glm5 = providers[0]
            .models
            .iter()
            .find(|m| m.id == "glm-5")
            .unwrap();
        // models.dev pricing won.
        assert_eq!(glm5.cost.as_ref().unwrap().input, 1.0);
        assert_eq!(glm5.cost.as_ref().unwrap().output, 3.2);
        // Empty convenience fields filled from models.dev.
        assert_eq!(
            providers[0].api_base_url.as_deref(),
            Some("https://api.z.ai/api/paas/v4")
        );
        // No temp file left behind.
        assert!(!catalog.with_file_name("catalog.json.tmp").exists());
    }

    #[test]
    fn test_check_clean_after_write() {
        // Self-consistent: write produces a synced catalog, then --check against
        // the same upstream must report clean (the merge is idempotent).
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("catalog.json");
        fs::write(&catalog, STALE_CATALOG).unwrap();
        apply(&catalog, parse_upstream(), false).unwrap();
        let code = apply(&catalog, parse_upstream(), true).unwrap();
        assert_eq!(code, ExitCode::SUCCESS);
    }
}
