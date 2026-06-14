//! Integration tests verifying that each feature flag compiles independently.

#[cfg(feature = "provider")]
#[test]
fn test_provider_feature() {
    let catalog = llm_kernel::provider::ProviderIndex::embedded();
    assert!(!catalog.ids().is_empty());
}

#[cfg(feature = "secrets")]
#[test]
fn test_secrets_feature() {
    let vault = llm_kernel::secrets::SecretVault::empty();
    assert!(vault.is_empty());
}

#[cfg(feature = "config")]
#[test]
fn test_config_feature() {
    let template = llm_kernel::config::default_config_template("test-product");
    assert!(template.contains("[llm]"));
}

#[cfg(feature = "store")]
#[test]
fn test_store_feature() {
    let conn = llm_kernel::store::init_in_memory("CREATE TABLE t(id INTEGER)");
    assert!(conn.is_ok());
}

#[cfg(feature = "provider")]
#[test]
fn test_prelude_reexports_provider() {
    let _catalog: &llm_kernel::prelude::ProviderIndex =
        llm_kernel::provider::ProviderIndex::embedded();
}

#[cfg(feature = "secrets")]
#[test]
fn test_prelude_reexports_secrets() {
    let _vault: llm_kernel::prelude::SecretVault = llm_kernel::secrets::SecretVault::empty();
}

#[cfg(feature = "provider")]
#[test]
fn test_catalog_has_models() {
    let catalog = llm_kernel::provider::ProviderIndex::embedded();
    let models = catalog.models_for("zai");
    assert!(!models.is_empty(), "zai provider should have models");

    let found = catalog.find_model("glm-5");
    assert!(found.is_some(), "should find glm-5 model");
}

#[cfg(feature = "graph-pool")]
#[tokio::test]
async fn test_graph_pool_feature() {
    let graph = llm_kernel::graph::AsyncPoolGraph::open_in_memory(2)
        .await
        .expect("open_in_memory should work");
    let stats = graph.stats().await.expect("stats on empty db");
    assert_eq!(stats.total_nodes, 0);
}

/// Compile-time check that `PgGraph` implements `GraphBackend` (no server).
#[cfg(feature = "graph-pg")]
#[test]
fn test_graph_pg_feature() {
    use llm_kernel::graph::{GraphBackend, PgGraph};
    fn _assert_impl<T: GraphBackend>() {}
    _assert_impl::<PgGraph>();
}

/// Compile-time check that `QdrantVectorIndex` implements `AsyncVectorIndex`.
#[cfg(feature = "qdrant")]
#[test]
fn test_qdrant_feature() {
    use llm_kernel::embedding::{AsyncVectorIndex, QdrantVectorIndex};
    fn _assert_impl<T: AsyncVectorIndex>() {}
    _assert_impl::<QdrantVectorIndex>();
}
