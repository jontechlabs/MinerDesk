//! Explicit LAN hints only. No credentials, enrollment, commands or trust changes.
use super::protocol::DiscoveredHub;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::{net::UdpSocket, sync::watch};
pub const DISCOVERY_PORT: u16 = 17990;
const PROTOCOL: &str = "minerdesk-fleet/1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    protocol: String,
    nonce: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    protocol: String,
    nonce: String,
    hub: DiscoveredHub,
}

pub async fn discover() -> Result<Vec<DiscoveredHub>, String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .await
        .map_err(|e| e.to_string())?;
    socket.set_broadcast(true).map_err(|e| e.to_string())?;
    discover_on(
        socket,
        SocketAddr::from((Ipv4Addr::BROADCAST, DISCOVERY_PORT)),
        Duration::from_millis(1200),
    )
    .await
}
async fn discover_on(
    socket: UdpSocket,
    target: SocketAddr,
    window: Duration,
) -> Result<Vec<DiscoveredHub>, String> {
    let nonce = uuid::Uuid::new_v4().to_string();
    let request = serde_json::to_vec(&Probe {
        protocol: PROTOCOL.into(),
        nonce: nonce.clone(),
    })
    .map_err(|e| e.to_string())?;
    socket
        .send_to(&request, target)
        .await
        .map_err(|e| e.to_string())?;
    let deadline = tokio::time::Instant::now() + window;
    let mut buffer = [0u8; 2048];
    let mut found = BTreeMap::new();
    while let Ok(Ok((size, _))) =
        tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer)).await
    {
        if size == buffer.len() {
            continue;
        }
        if let Ok(answer) = serde_json::from_slice::<Answer>(&buffer[..size]) {
            if answer.protocol == PROTOCOL
                && answer.nonce == nonce
                && answer.hub.name.len() <= 128
                && answer.hub.fleet_id.len() <= 64
                && answer.hub.address.len() <= 256
                && answer.hub.address.starts_with("https://")
                && answer.hub.certificate_fingerprint.len() == 64
            {
                found.insert(answer.hub.fleet_id.clone(), answer.hub);
                if found.len() >= 32 {
                    break;
                }
            }
        }
    }
    Ok(found.into_values().collect())
}
pub(crate) async fn responder(
    info: DiscoveredHub,
    ip: IpAddr,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let socket = UdpSocket::bind(SocketAddr::new(ip, DISCOVERY_PORT))
        .await
        .map_err(|e| e.to_string())?;
    respond_on(socket, info, &mut stop).await
}
async fn respond_on(
    socket: UdpSocket,
    info: DiscoveredHub,
    stop: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let mut buffer = [0u8; 512];
    let mut last = tokio::time::Instant::now() - Duration::from_secs(1);
    loop {
        tokio::select! {
            _=stop.changed()=>return Ok(()),
            result=socket.recv_from(&mut buffer)=> {
                let (size,peer)=result.map_err(|e|e.to_string())?;
                if size==buffer.len() || peer.ip().is_multicast() || last.elapsed()<Duration::from_millis(50){continue;}
                let Ok(probe)=serde_json::from_slice::<Probe>(&buffer[..size]) else{continue;};
                if probe.protocol!=PROTOCOL || probe.nonce.len()!=36 {continue;}
                let response=serde_json::to_vec(&Answer{protocol:PROTOCOL.into(),nonce:probe.nonce,hub:info.clone()}).map_err(|e|e.to_string())?;
                if response.len()>1024{continue;}
                socket.send_to(&response,peer).await.map_err(|e|e.to_string())?;last=tokio::time::Instant::now();
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn explicit_loopback_discovery_returns_only_public_hint() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let info = DiscoveredHub {
            fleet_id: "fixture".into(),
            name: "Test hub".into(),
            address: "https://localhost:17989".into(),
            certificate_fingerprint: "a".repeat(64),
        };
        let (stop, mut receiver) = watch::channel(false);
        let worker = tokio::spawn(async move {
            respond_on(server, info, &mut receiver).await.unwrap();
        });
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let found = discover_on(client, address, Duration::from_millis(150))
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].fleet_id, "fixture");
        let encoded = serde_json::to_string(&found).unwrap();
        assert!(!encoded.contains("token"));
        assert!(!encoded.contains("key"));
        let _ = stop.send(true);
        worker.await.unwrap();
    }
}
