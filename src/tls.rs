//! TLS crypto provider bootstrap for the `rustls-ring` feature.
//!
//! reqwest 0.13's `rustls-no-provider` feature compiles no crypto provider
//! into the binary and panics at `Client` build time unless a process-default
//! provider is installed first. The `rustls-ring` feature relies on that
//! escape hatch (see #93), so every HTTP client construction site calls
//! [`ensure_tls_provider`] first. The install is idempotent: an already
//! installed provider (e.g. the application's own choice) wins, and the
//! `Err` from the redundant install is ignored.

#[cfg(feature = "rustls-ring")]
pub(crate) fn ensure_tls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// No-op without `rustls-ring`: the `rustls-aws-lc-rs` (default) path ships
/// the aws-lc-rs provider inside reqwest and needs no runtime install.
#[cfg(not(feature = "rustls-ring"))]
pub(crate) fn ensure_tls_provider() {}

#[cfg(all(test, feature = "rustls-ring", feature = "client-async"))]
mod tests {
    use super::ensure_tls_provider;

    /// Regression test for the #93 panic path: under `rustls-no-provider`,
    /// building a client without an installed provider panics inside reqwest.
    /// llm-kernel must install the ring provider first.
    #[test]
    fn client_builds_with_ring_provider() {
        ensure_tls_provider();
        let client = reqwest::Client::builder().build();
        assert!(
            client.is_ok(),
            "reqwest client must build after ring provider install"
        );
    }
}
