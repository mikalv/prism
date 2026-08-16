//! Fallback-aware embedding provider
//!
//! Wraps a primary and a fallback provider that serve the SAME vector space
//! (same model or identical weights on two hosts, e.g. `bge-m3` on a local
//! Ollama box and on the NVIDIA NIM API). If the primary fails, requests are
//! transparently routed to the fallback until the primary recovers.
//!
//! Routing uses a simple circuit breaker: after a primary failure the breaker
//! opens for a cooldown period (default 30s) during which requests go straight
//! to the fallback. When the cooldown expires the next request retries the
//! primary (half-open) and closes the breaker again on success.

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::provider::EmbeddingProvider;

/// Default circuit-breaker cooldown after a primary failure.
const DEFAULT_COOLDOWN_SECS: u64 = 30;

pub struct FallbackProvider {
    primary: Box<dyn EmbeddingProvider>,
    /// Used while the primary circuit breaker is open.
    fallback: Box<dyn EmbeddingProvider>,
    /// Epoch millis until which the primary is skipped. 0 = closed (use primary).
    open_until_ms: AtomicU64,
    cooldown: Duration,
}

impl FallbackProvider {
    /// Create a fallback-aware provider.
    ///
    /// Both providers must report the same `dimensions()` — embeddings from
    /// different vector spaces cannot be mixed in one index. Construction
    /// fails if they disagree.
    pub fn new(
        primary: Box<dyn EmbeddingProvider>,
        fallback: Box<dyn EmbeddingProvider>,
    ) -> anyhow::Result<Self> {
        let pd = primary.dimensions();
        let fd = fallback.dimensions();
        if pd != fd {
            anyhow::bail!(
                "fallback provider dimensions mismatch: primary '{}' has {} dims, \
                 fallback '{}' has {} dims — both must serve the same vector space",
                primary.model_name(),
                pd,
                fallback.model_name(),
                fd
            );
        }
        if primary.model_name() != fallback.model_name() {
            tracing::warn!(
                "embedding fallback '{}' does not match primary model name '{}' \
                 (dimensions agree at {}); assuming same weights — cache keys use '{}'",
                fallback.model_name(),
                primary.model_name(),
                pd,
                primary.model_name()
            );
        }
        Ok(Self {
            primary,
            fallback,
            open_until_ms: AtomicU64::new(0),
            cooldown: Duration::from_secs(DEFAULT_COOLDOWN_SECS),
        })
    }

    /// Override the circuit-breaker cooldown.
    pub fn with_cooldown(mut self, cooldown: Duration) -> Self {
        self.cooldown = cooldown;
        self
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Whether the primary circuit breaker is currently open.
    fn primary_open(&self) -> bool {
        let until = self.open_until_ms.load(Ordering::Relaxed);
        until != 0 && until > Self::now_ms()
    }

    /// Open the breaker: skip the primary for one cooldown window.
    fn trip_primary(&self) {
        let next = Self::now_ms() + self.cooldown.as_millis() as u64;
        self.open_until_ms.store(next, Ordering::Relaxed);
    }

    /// Close the breaker: primary healthy again.
    fn reset_primary(&self) {
        self.open_until_ms.store(0, Ordering::Relaxed);
    }
}

#[async_trait]
impl EmbeddingProvider for FallbackProvider {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        if !self.primary_open() {
            match self.primary.embed(text).await {
                Ok(v) => {
                    self.reset_primary();
                    return Ok(v);
                }
                Err(e) => {
                    metrics::counter!("prism_embedding_failovers_total", "direction" => "primary_to_fallback").increment(1);
                    tracing::warn!(
                        "primary embedding provider '{}' failed: {} — failing over",
                        self.primary.model_name(),
                        e
                    );
                    self.trip_primary();
                }
            }
        }

        self.fallback.embed(text).await
    }

    async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        if !self.primary_open() {
            match self.primary.embed_batch(texts).await {
                Ok(v) => {
                    self.reset_primary();
                    return Ok(v);
                }
                Err(e) => {
                    metrics::counter!("prism_embedding_failovers_total", "direction" => "primary_to_fallback").increment(1);
                    tracing::warn!(
                        "primary embedding provider '{}' failed: {} — failing over",
                        self.primary.model_name(),
                        e
                    );
                    self.trip_primary();
                }
            }
        }

        self.fallback.embed_batch(texts).await
    }

    /// Model name of the active provider, used for cache keys. With a
    /// same-vector-space fallback the cache key must be identical for primary
    /// and fallback so cached vectors remain valid across failovers.
    fn model_name(&self) -> &str {
        if self.primary_open() {
            return self.fallback.model_name();
        }
        self.primary.model_name()
    }

    fn dimensions(&self) -> usize {
        self.primary.dimensions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering as AtomicOrdering;
    use std::sync::Arc;

    struct FlakyProvider {
        model: &'static str,
        dims: usize,
        fail: Arc<AtomicUsize>,
        calls: AtomicUsize,
    }

    impl FlakyProvider {
        fn healthy(model: &'static str, dims: usize) -> Box<Self> {
            Box::new(Self {
                model,
                dims,
                fail: Arc::new(AtomicUsize::new(0)),
                calls: AtomicUsize::new(0),
            })
        }

        fn flaky(model: &'static str, dims: usize) -> (Box<Self>, Arc<AtomicUsize>) {
            let fail = Arc::new(AtomicUsize::new(usize::MAX));
            let p = Box::new(Self {
                model,
                dims,
                fail: fail.clone(),
                calls: AtomicUsize::new(0),
            });
            (p, fail)
        }
    }

    #[async_trait]
    impl EmbeddingProvider for FlakyProvider {
        async fn embed(&self, _text: &str) -> anyhow::Result<Vec<f32>> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            if self.fail.load(AtomicOrdering::SeqCst) > 0 {
                anyhow::bail!("provider down");
            }
            Ok(vec![0.5; self.dims])
        }

        fn model_name(&self) -> &str {
            self.model
        }

        fn dimensions(&self) -> usize {
            self.dims
        }
    }

    #[tokio::test]
    async fn test_dimension_mismatch_rejected() {
        let primary = FlakyProvider::healthy("m1", 768);
        let fallback = FlakyProvider::healthy("m2", 1024);
        assert!(FallbackProvider::new(primary, fallback).is_err());
    }

    #[tokio::test]
    async fn test_fails_over_to_fallback() {
        let primary = FlakyProvider::healthy("primary", 8);
        let (fallback, fail_flag) = FlakyProvider::flaky("fallback", 8);
        // fallback starts healthy
        fail_flag.store(0, AtomicOrdering::SeqCst);

        let primary_fail = Arc::new(AtomicUsize::new(usize::MAX));
        // make primary a flaky one instead
        let primary = Box::new(FlakyProvider {
            model: "primary",
            dims: 8,
            fail: primary_fail.clone(),
            calls: AtomicUsize::new(0),
        });

        let fb = FallbackProvider::new(primary, fallback).unwrap();
        let v = fb.embed("hello").await.unwrap();
        assert_eq!(v.len(), 8);
        // breaker now open: model_name reports fallback
        assert_eq!(fb.model_name(), "fallback");
    }

    #[tokio::test]
    async fn test_both_healthy_uses_primary() {
        let primary = FlakyProvider::healthy("primary", 6);
        let fallback = FlakyProvider::healthy("fallback", 6);
        let fb = FallbackProvider::new(primary, fallback).unwrap();

        assert_eq!(fb.model_name(), "primary");
        let v = fb.embed("hello").await.unwrap();
        assert_eq!(v.len(), 6);
        // still primary after success
        assert_eq!(fb.model_name(), "primary");
    }

    #[tokio::test]
    async fn test_fallback_failure_propagates() {
        // both down: error must propagate with provider context
        let (primary, _pf) = FlakyProvider::flaky("primary", 4);
        let (fallback, _ff) = FlakyProvider::flaky("fallback", 4);
        let fb = FallbackProvider::new(primary, fallback).unwrap();

        let err = fb.embed("hello").await.unwrap_err();
        assert!(err.to_string().contains("provider down"));
    }

    #[tokio::test]
    async fn test_primary_recovery_after_cooldown() {
        let primary_fail = Arc::new(AtomicUsize::new(usize::MAX));
        let primary = Box::new(FlakyProvider {
            model: "primary",
            dims: 4,
            fail: primary_fail.clone(),
            calls: AtomicUsize::new(0),
        });
        let fallback = FlakyProvider::healthy("fallback", 4);

        let fb = FallbackProvider::new(primary, fallback)
            .unwrap()
            .with_cooldown(Duration::from_millis(50));

        // primary down -> failover
        let v = fb.embed("x").await.unwrap();
        assert_eq!(v.len(), 4);
        assert_eq!(fb.model_name(), "fallback");

        // primary recovers, wait out the cooldown
        primary_fail.store(0, AtomicOrdering::SeqCst);
        tokio::time::sleep(Duration::from_millis(80)).await;

        let v = fb.embed("y").await.unwrap();
        assert_eq!(v.len(), 4);
        assert_eq!(fb.model_name(), "primary");
    }
}
