#![doc = include_str!("../README.md")]

mod crypto;
mod dht;

#[cfg(feature = "iroh-gossip")]
mod gossip;
#[cfg(feature = "iroh-gossip")]
pub use gossip::{
    AutoDiscoveryGossip, Bootstrap, BubbleMerge, GossipReceiver, GossipRecordContent, GossipSender,
    MessageOverlapMerge, Publisher, Topic, TopicId,
};

pub use crypto::{
    DefaultSecretRotation, EncryptedRecord, Record, RecordPublisher, RecordTopic, RotationHandle,
    SecretRotation, encryption_keypair, node_slot, salt, signing_keypair,
};
pub use dht::Dht;

/// Number of independent DHT slots per (topic, minute) combination.
///
/// Nodes are distributed across slots based on a hash of their node ID,
/// reducing write collisions from N-to-1 to ~N/DHT_SLOTS-to-1.
pub const DHT_SLOTS: usize = 8;

/// Get the current Unix minute timestamp, optionally offset.
///
/// # Arguments
///
/// * `minute_offset` - Offset in minutes from now (can be negative)
///
/// # Example
///
/// ```ignore
/// let now = unix_minute(0);
/// let prev_minute = unix_minute(-1);
/// ```
pub fn unix_minute(minute_offset: i64) -> u64 {
    ((chrono::Utc::now().timestamp() as f64 / 60.0f64).floor() as i64 + minute_offset) as u64
}
