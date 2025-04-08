use futures::prelude::*;
use libp2p::{core::upgrade, gossipsub, gossipsub::{MessageAuthenticity, ValidationMode}, identify, identity, kad, kad::{store::MemoryStore}, mdns, noise, request_response, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux, Multiaddr, PeerId, 
             StreamProtocol, Swarm, Transport};
use std::{
    collections::hash_map::DefaultHasher,
    error::Error,
    hash::{Hash, Hasher},
    io,
    task::Poll,
    time::Duration,
};
use futures::channel::mpsc::{Receiver, Sender};
use libp2p::swarm::handler::ProtocolSupport;
use libp2p::swarm::SwarmEvent::Behaviour;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

struct Command {

}

#[derive(Clone)]
pub struct Client {
    sender: Sender<Command>,
}

async fn new(secret_key_seed: Option<u8>) -> Result<(Client, impl Stream<Item = Event>, EventLoop), Box<dyn Error>> {
    let id_keys = match secret_key_seed {
        Some(seed) => {
            let mut bytes = [0u8; 32];
            bytes[0] = seed;
            identity::Keypair::ed25519_from_bytes(bytes).unwrap()
        }
        None => identity::Keypair::generate_ed25519(),
    };
    let peer_id  = id_keys.public().to_peer_id();

    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(10))
        .validation_mode(gossipsub::ValidationMode::Strict)
        .message_id_fn(message_id_fn)
        .build()
        .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg))?;

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(id_keys)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| Ok(MyBehaviour {
            gossipsub: gossipsub::Behaviour::new(
                MessageAuthenticity::Signed(key.clone()),
                gossipsub_config
            )?,
            kademlia: kad::Behaviour::new(
                peer_id,
                MemoryStore::new(key.public().to_peer_id())
            ),
            mdns: mdns::tokio::Behaviour::new(
                mdns::Config::default(), key.public().to_peer_id()).unwrap(),
            identify: identify::Behaviour::new(
                identify::Config::new("/ipfs/1.0.0".to_string(), key.public()),
            ),
            file_transfer: request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/file-exchange/1"),
                    request_response::ProtocolSupport::Full
                    )],
                request_response::Config::default(),
            ),
        }))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));
    let (command_sender, command_receiver) = futures::channel::mpsc::channel(0);
    let (event_sender, event_receiver) = futures::channel::mpsc::channel(0);

    Ok((
        Client{
            sender: command_sender,
        },
        event_receiver,
        EventLoop::new(swarm, command_receiver, event_sender)
        ))
}

fn message_id_fn(message: &gossipsub::Message) -> gossipsub::MessageId {
    let mut s = DefaultHasher::new();
    message.data.hash(&mut s);
    gossipsub::MessageId::from(s.finish().to_string())
}

enum Event {

}

struct EventLoop {

}

impl EventLoop {
    fn new(p0: Swarm<MyBehaviour>, p1: Receiver<Command>, p2: Sender<Event>) -> EventLoop {
        todo!()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileRequest(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileResponse(String);

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    mdns: mdns::tokio::Behaviour,
    identify: identify::Behaviour,
    file_transfer: request_response::cbor::Behaviour<FileRequest, FileResponse>
}