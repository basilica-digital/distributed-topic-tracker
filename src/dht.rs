//! Mainline BitTorrent DHT client for mutable record operations.
//!
//! Provides async interface for DHT get/put operations with automatic
//! retry logic and lazy connection initialization.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use ed25519_dalek::VerifyingKey;
use futures_lite::StreamExt;
use mainline::MutableItem;

const RETRY_DEFAULT: usize = 3;

/// DHT client wrapper with lazy initialization and concurrent access.
///
/// Manages a shared connection to the mainline DHT. The underlying
/// `AsyncDht` is `Clone` and all methods take `&self`, so multiple
/// concurrent queries are supported.
#[derive(Debug, Clone)]
pub struct Dht {
    inner: Arc<tokio::sync::OnceCell<mainline::async_dht::AsyncDht>>,
}

impl Dht {
    /// Create a new DHT client.
    ///
    /// The actual mainline DHT connection is established lazily on first use.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    /// Get or initialize the underlying AsyncDht client.
    async fn client(&self) -> Result<mainline::async_dht::AsyncDht> {
        let dht = self
            .inner
            .get_or_try_init(|| async {
                mainline::Dht::builder()
                    .build()
                    .map(|d| d.as_async())
                    .map_err(|e| anyhow::anyhow!(e))
            })
            .await?;
        Ok(dht.clone())
    }

    /// Retrieve mutable records from the DHT.
    ///
    /// # Arguments
    ///
    /// * `pub_key` - Ed25519 public key for the record
    /// * `salt` - Optional salt for record lookup
    /// * `more_recent_than` - Sequence number filter (get records newer than this)
    /// * `timeout` - Maximum time to wait for results
    pub async fn get(
        &self,
        pub_key: VerifyingKey,
        salt: Option<Vec<u8>>,
        more_recent_than: Option<i64>,
        timeout: Duration,
    ) -> Result<Vec<MutableItem>> {
        let dht = self.client().await?;
        Ok(tokio::time::timeout(
            timeout,
            dht.get_mutable(pub_key.as_bytes(), salt.as_deref(), more_recent_than)
                .collect::<Vec<_>>(),
        )
        .await?)
    }

    /// Publish a mutable record to the DHT.
    ///
    /// # Arguments
    ///
    /// * `signing_key` - Ed25519 secret key for signing
    /// * `pub_key` - Ed25519 public key (used for routing)
    /// * `salt` - Optional salt for record slot
    /// * `data` - Record value to publish
    /// * `retry_count` - Number of retry attempts (default: 3)
    /// * `timeout` - Per-request timeout
    pub async fn put_mutable(
        &self,
        signing_key: mainline::SigningKey,
        pub_key: VerifyingKey,
        salt: Option<Vec<u8>>,
        data: Vec<u8>,
        retry_count: Option<usize>,
        timeout: Duration,
    ) -> Result<()> {
        let retries = retry_count.unwrap_or(RETRY_DEFAULT);

        for i in 0..retries {
            let dht = self.client().await?;

            let most_recent_result = tokio::time::timeout(
                timeout,
                dht.get_mutable_most_recent(pub_key.as_bytes(), salt.as_deref()),
            )
            .await?;

            let item = if let Some(mut_item) = most_recent_result {
                MutableItem::new(
                    signing_key.clone(),
                    &data,
                    mut_item.seq() + 1,
                    salt.as_deref(),
                )
            } else {
                MutableItem::new(signing_key.clone(), &data, 0, salt.as_deref())
            };

            let put_result = match tokio::time::timeout(
                Duration::from_secs(10),
                dht.put_mutable(item.clone(), Some(item.seq())),
            )
            .await
            {
                Ok(result) => result.ok(),
                Err(_) => None,
            };

            if put_result.is_some() {
                break;
            } else if i == retries - 1 {
                bail!("failed to publish record")
            }

            tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 2000)).await;
        }
        Ok(())
    }
}

impl Default for Dht {
    fn default() -> Self {
        Self::new()
    }
}
