use futures::prelude::*;
use libp2p::{gossipsub, gossipsub::{MessageAuthenticity}, identify, identity, kad, 
             kad::{store::MemoryStore}, mdns, noise, request_response, 
             swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux, PeerId, StreamProtocol, Swarm, Transport};
use std::{
    collections::hash_map::DefaultHasher,
    error::Error,
    hash::{Hash, Hasher},
    io,
    time::Duration,
};
use std::collections::{HashMap};
use std::time::{SystemTime, UNIX_EPOCH};
use futures::channel::mpsc::{Receiver, Sender};
use futures::channel::{mpsc, oneshot};
use libp2p::kad::{QueryResult, Quorum};
use libp2p::request_response::ResponseChannel;
use libp2p::swarm::SwarmEvent::Behaviour;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
struct PeerNotRegisteredError {
    message: String,
}
impl std::fmt::Display for PeerNotRegisteredError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl Error for PeerNotRegisteredError {}

enum Command {
    SendMessage {
        message: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    Register {
        user_info: UserInfo,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    ChangeTopic {
        topic: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    Connect {
        address: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserInfo {
    peer_id: PeerId,
    username: String
}

#[derive(Clone)]
pub struct Client {
    sender: Sender<Command>,
    peer_id: PeerId,
}

impl Client {
    pub async fn send_message(&mut self, message: String) -> Result<(), Box<dyn Error + Send>>{
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::SendMessage {message, sender})
            .await
            .expect("Command receiver not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }

    pub async fn register(&mut self, username: String) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        let user_info = UserInfo { peer_id: self.peer_id, username };
        self.sender
            .send(Command::Register {user_info, sender})
            .await
            .expect("Command receiver not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }

    pub async fn change_topic(&mut self, topic: String) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::ChangeTopic {topic, sender})
            .await
            .expect("Command not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }

    pub async fn connect(&mut self, address: String) -> Result<(), Box<dyn Error + Send>>{
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::Connect {address, sender})
            .await
            .expect("Command receiver not to be dropped");
        receiver.await.expect("Sender not to be dropped")
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
            peer_id,

        },
        event_receiver,
        EventLoop::new(swarm, command_receiver, event_sender, )
        ))
}

fn message_id_fn(message: &gossipsub::Message) -> gossipsub::MessageId {
    let mut s = DefaultHasher::new();
    let mut timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_be_bytes()
        .to_vec();
    timestamp.append(&mut message.data.clone());
    timestamp.hash(&mut s);
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
    topic: gossipsub::IdentTopic,
    registered_users: HashMap<PeerId, String>,
}

impl EventLoop {
    fn new(swarm: Swarm<MyBehaviour>, command_receiver: Receiver<Command>, event_sender: Sender<Event>) -> Self {
        Self {
            swarm,
            command_receiver,
            event_sender,
            topic: gossipsub::IdentTopic::new("default"),
            registered_users: HashMap::new(),
        }
    }

    pub async fn run(mut self) {
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
                message})) => {
                if !self.registered_users.contains_key(&peer_id) {
                    let key = kad::RecordKey::new(&peer_id.to_bytes());
                    self.swarm.behaviour_mut().kademlia.get_record(key);
                } else {
                    let username = self.registered_users.get(&peer_id).unwrap();
                    println!("{username}: {}",
                             String::from_utf8_lossy(&message.data));
                }
            },
            
            // Query result to fetch the username associated with a given peer id.
            Behaviour(MyBehaviourEvent::Kademlia(kad::Event::OutboundQueryProgressed {result, ..})) => {
                match result {
                    QueryResult::GetRecord(Ok(kad::GetRecordOk::FoundRecord(kad::PeerRecord {
                        record: kad::Record {key, value, ..},
                        .. }))) => {
                        let peer_id = PeerId::from_bytes(key.as_ref()).unwrap();
                        let username = match String::from_utf8(value.clone()) {
                            Ok(name) => {name}
                            Err(e) => {
                                println!("Failed to decode username withe error {e}");
                                "Default".to_string()
                            }
                        };
                        self.registered_users.insert(peer_id, username);
                    }
                    QueryResult::Bootstrap(_) => {}
                    QueryResult::GetClosestPeers(_) => {}
                    QueryResult::GetProviders(_) => {}
                    QueryResult::StartProviding(_) => {}
                    QueryResult::RepublishProvider(_) => {}
                    QueryResult::PutRecord(_) => {}
                    QueryResult::RepublishRecord(_) => {},
                    _ => {}
                }
            },
            SwarmEvent::NewListenAddr {address, ..} => {
                println!("Local node is listening on {address}")
            }
            _ => {}
        }
    }



    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::SendMessage {message, sender} => {
                if !self.registered_users.contains_key(self.swarm.local_peer_id()) {
                    let error = PeerNotRegisteredError {
                        message: "User must register first".to_string()
                    };
                    sender.send(Err(Box::new(error))).expect("Sender failed");
                    return;
                }
                match self.swarm.behaviour_mut().gossipsub.publish(self.topic.clone(), message.as_bytes()) {
                    Ok(_) => {
                        println!("Sent message: {message}");
                        sender.send(Ok(())).expect("Sender failed");
                    },
                    Err(e) => {
                        println!("Failed to send message: {e:?}");
                        sender.send(Err(Box::new(e))).expect("Sender failed");
                    }
                }
            },
            Command::Register {user_info, sender} => {
                let record = kad::Record {
                    key: kad::RecordKey::new(&user_info.peer_id.to_bytes()),
                    value: user_info.username.clone().into_bytes(),
                    publisher: None,
                    expires: None
                };
                match self.swarm.behaviour_mut().kademlia.put_record(record, Quorum::One) {
                    Ok(_) => {
                        println!("Successfully registered as {}", user_info.username.as_str());
                        self.registered_users.insert(user_info.peer_id, user_info.username);
                        sender.send(Ok(())).expect("Sender failed");
                    }
                    Err(e) => {
                        println!("Failed to register withe err: {e}");
                        sender.send(Err(Box::new(e))).expect("Sender failed");
                    }
                }
            },
            Command::ChangeTopic {topic, sender} => {
                let topic_hash = gossipsub::IdentTopic::new(topic.clone());
                self.swarm.behaviour_mut().gossipsub.unsubscribe(&self.topic);
                match self.swarm.behaviour_mut().gossipsub.subscribe(&topic_hash) {
                    Ok(_) => {
                        println!("Subscribed to topic {}", topic);
                        self.topic = topic_hash;
                        sender.send(Ok(())).expect("Sender failed");
                    },
                    Err(e) => {
                        println!("Failed to subscribe to topic");
                        sender.send(Err(Box::new(e))).expect("Sender failed");
                    }
                }
            },
            Command::Connect {address, sender} => {
                match self.swarm.listen_on(address.parse().unwrap()) {
                    Ok(_) => {
                        println!("Listening on address {}", address);
                        sender.send(Ok(())).expect("Sender failed");
                    },
                    Err(e) => {
                        println!("Failed to listen on address");
                        sender.send(Err(Box::new(e))).expect("Sender failed");
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