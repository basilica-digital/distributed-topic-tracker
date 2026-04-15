//! Background publisher that updates DHT records with active peer info.

use actor_helper::{Action, Actor, Handle, Receiver};
use std::time::Duration;

use crate::{GossipReceiver, RecordPublisher};
use anyhow::Result;

/// Periodically publishes node state to DHT for peer discovery.
///
/// Publishes a record after an initial 10s delay, then repeatedly with
/// randomized 0-49s intervals, containing this node's active neighbor list
/// and message hashes for bubble detection and merging.
#[derive(Debug, Clone)]
pub struct Publisher {
    _api: Handle<PublisherActor, anyhow::Error>,
}

#[derive(Debug)]
struct PublisherActor {
    rx: Receiver<Action<PublisherActor>>,

    record_publisher: RecordPublisher,
    gossip_receiver: GossipReceiver,
    ticker: tokio::time::Interval,
}

impl Publisher {
    /// Create a new background publisher.
    ///
    /// Spawns a background task that periodically publishes records.
    pub fn new(record_publisher: RecordPublisher, gossip_receiver: GossipReceiver) -> Result<Self> {
        let (api, rx) = Handle::channel();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(10));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut actor = PublisherActor {
                rx,
                record_publisher,
                gossip_receiver,
                ticker,
            };
            let _ = actor.run().await;
        });

        Ok(Self { _api: api })
    }
}

impl Actor<anyhow::Error> for PublisherActor {
    async fn run(&mut self) -> Result<()> {
        tracing::debug!("Publisher: starting publisher actor");
        loop {
            tokio::select! {
                Ok(action) = self.rx.recv_async() => {
                    action(self).await;
                }
                _ = self.ticker.tick() => {
                    tracing::debug!("Publisher: tick fired, attempting to publish");
                    let _ = self.publish().await;
                    let next_interval = rand::random::<u64>() % 50;
                    tracing::debug!("Publisher: next publish in {}s", next_interval);
                    self.ticker.reset_after(Duration::from_secs(next_interval));
                }
                else => break Ok(()),
            }
        }
    }
}

impl PublisherActor {
    async fn publish(&mut self) -> Result<()> {
        let unix_minute = crate::unix_minute(0);

        let mut active_peers: Vec<[u8; 32]> = self
            .gossip_receiver
            .neighbors()
            .await
            .iter()
            .filter_map(|pub_key| TryInto::<[u8; 32]>::try_into(pub_key.as_slice()).ok())
            .collect();
        active_peers.truncate(5);
        active_peers.resize(5, [0u8; 32]);

        let mut last_message_hashes: Vec<[u8; 32]> =
            self.gossip_receiver.last_message_hashes().await;
        last_message_hashes.truncate(5);
        last_message_hashes.resize(5, [0u8; 32]);

        tracing::debug!(
            "Publisher: publishing record for unix_minute {} with {} active_peers and {} message_hashes",
            unix_minute,
            active_peers.iter().filter(|p| **p != [0u8; 32]).count(),
            last_message_hashes
                .iter()
                .filter(|h| **h != [0u8; 32])
                .count()
        );

        let record_content = crate::gossip::GossipRecordContent {
            active_peers: active_peers.try_into().expect("padded to exactly 5"),
            last_message_hashes: last_message_hashes.try_into().expect("padded to exactly 5"),
        };

        tracing::debug!("Publisher: created record content: {:?}", record_content);

        let res = self
            .record_publisher
            .new_record(unix_minute, record_content);
        tracing::debug!("Publisher: created new record: {:?}", res);
        let record = res?;
        let result = self.record_publisher.publish_record(record).await;

        if result.is_ok() {
            tracing::debug!("Publisher: successfully published record");
        } else {
            tracing::debug!("Publisher: failed to publish record: {:?}", result);
        }

        result
    }
}
