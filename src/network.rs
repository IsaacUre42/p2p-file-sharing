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
use std::collections::HashSet;
use futures::channel::mpsc::{Receiver, Sender};
use futures::channel::{mpsc, oneshot};
use libp2p::request_response::ResponseChannel;
use libp2p::swarm::handler::ProtocolSupport;
use libp2p::swarm::PeerAddresses;
use libp2p::swarm::SwarmEvent::Behaviour;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

enum Command {
    SendMessage {
        message: String,
        sender: oneshot::Sender<(), Box<dyn Error + Send>>
    }
}

#[derive(Clone)]
pub struct Client {
    sender: Sender<Command>,
}

impl Client {
    pub async fn send_message(&mut self, message: String) {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::SendMessage {message, sender})
            .await
            .expect("Command receiver not to be dropped");
        receiver.await.expect("Sender not to be dropped");

    }
}

pub async fn new(secret_key_seed: Option<u8>) -> Result<(Client, impl Stream<Item = Event>, EventLoop), Box<dyn Error>> {
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
    let (command_sender, command_receiver) = mpsc::channel(0);
    let (event_sender, event_receiver) = mpsc::channel(0);

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

pub enum Event {
    InboundRequest {
        request: String,
        channel: ResponseChannel<FileResponse>
    }
}

pub struct EventLoop {
    swarm: Swarm<MyBehaviour>,
    command_receiver: Receiver<Command>,
    event_sender: Sender<Event>,
    topic: gossipsub::IdentTopic
}

impl EventLoop {
    fn new(swarm: Swarm<MyBehaviour>, command_receiver: Receiver<Command>, event_sender: Sender<Event>) -> Self {
        Self {
            swarm,
            command_receiver,
            event_sender,
            topic: gossipsub::IdentTopic::new("test-net"),
        }
    }

    pub async fn run(mut self) {
        self.swarm.behaviour_mut().gossipsub.subscribe(&self.topic).unwrap();
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,
                command = self.command_receiver.next() => match command {
                    Some(c) => self.handle_command(c).await,
                    None => return
                }
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<MyBehaviourEvent>) {
        match event {
            Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                for (peer_id, _multiaddr) in list {
                    println!("mDNS discovered a new peer: {peer_id}");
                    self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                }
            },
            Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                for (peer_id, _multiaddr) in list {
                    println!("mDNS discover peer has expired: {peer_id}");
                    self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                }
            },
            Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: peer_id,
                message_id: id,
                message})) =>  println!(
                "Got message: '{}' with id: {id} from peer: {peer_id}",
                String::from_utf8_lossy(&message.data)
            ),
            SwarmEvent::NewListenAddr {address, ..} => {
                println!("Local node is listening on {address}")
            }
            _ => {}
        }
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::SendMessage {message, sender} => {
                match self.swarm.behaviour_mut().gossipsub.publish(self.topic.clone(), message.as_bytes()) {
                    Ok(_) => {
                        println!("Sent message: {message}");
                        sender.send(Ok(()));
                    },
                    Err(e) => {
                        println!("Failed to send message: {e:?}");
                        sender.send(Err(Box::new(e)));
                    }
                }
            }
        }
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