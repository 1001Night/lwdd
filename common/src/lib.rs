use serde::{Deserialize, Serialize};
use std::net::IpAddr;

pub const DISCOVERY_PORT: u16 = 63200;
pub const DNS_PORT: u16 = 53;
pub const HEARTBEAT_INTERVAL_SECS: u64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    DiscoveryRequest,
    DiscoveryResponse,
    Heartbeat { hostname: String, ip: IpAddr },
    HeartbeatAck,
}

impl Message {
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}
