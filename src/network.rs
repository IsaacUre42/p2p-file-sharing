use futures::prelude::*;
use libp2p::{gossipsub, gossipsub::{MessageAuthenticity}, identify, identity, kad, kad::{store::MemoryStore}, mdns, noise, rendezvous, request_response, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux, Multiaddr, PeerId, StreamProtocol, Swarm, Transport};
use std::{collections::hash_map::DefaultHasher, error::Error, fs, hash::{Hash, Hasher}, io, time::Duration};
use std::collections::{hash_map, HashMap};
use std::num::{NonZero};
use std::str::FromStr;
use std::string::{ToString};
use std::time::{SystemTime, UNIX_EPOCH};
use futures::channel::mpsc::{Receiver, Sender};
use futures::channel::{mpsc, oneshot};
use libp2p::kad::{QueryResult, Quorum};
use libp2p::multiaddr::Protocol;
use libp2p::rendezvous::Namespace;
use libp2p::request_response::{Message, OutboundRequestId, ResponseChannel};
use libp2p::swarm::PeerAddresses;
use libp2p::swarm::SwarmEvent::Behaviour;
use serde::{Deserialize, Serialize};
use crate::network::RequestResponseType::{MessageText, Trade};

#[derive(Debug)]
struct CustomError {
    message: String,
}
impl std::fmt::Display for CustomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl Error for CustomError {}

const DEFAULT_NAMESPACE: &str = "p2p-file-sharing";
const SERVER_PEER_ID: &str = "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

enum Command {
    SendMessage {
        message: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    Register {
        user_info: UserInfo,
        sender: oneshot::Sender<Result<String, Box<dyn Error + Send>>>
    },
    ChangeTopic {
        topic: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    Connect {
        address: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    Dial {
        username: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    SendRequest {
        request: PrivateRequest,
        username: String,
        sender: oneshot::Sender<Result<PrivateResponse, Box<dyn Error + Send>>>
    },
    OfferFiles {
        filenames: Vec<String>,
        files: HashMap<String, Vec<u8>>,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    GetOfferings {
        username: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    TradeResponse {
        response: bool,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    },
    SendFile {
        username: String,
        filename: String,
        sender: oneshot::Sender<Result<PrivateResponse, Box<dyn Error + Send>>>
    },
    ConnectRendezvous {
        server_address: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserInfo {
    peer_id: PeerId,
    username: String,
}

#[derive(Clone)]
pub struct Client {
    sender: Sender<Command>,
    peer_id: PeerId,
    name: String
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

    pub async fn register(&mut self, username: String) -> Result<String, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        let user_info = UserInfo { peer_id: self.peer_id, username};
        self.sender
            .send(Command::Register {user_info, sender})
            .await
            .expect("Command receiver not to be dropped");
        let received  = receiver.await.expect("Sender not to be dropped");
        match received {
            Ok(ref name) => {self.name = name.to_string()}
            Err(_) => {}
        }
        received
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
    
    pub async fn dial(&mut self, username: String) -> Result<(), Box<dyn Error + Send>>{
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::Dial {username, sender})
            .await
            .expect("Command not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }
    
    pub async fn dm(&mut self, username: String, message: String) -> Result<PrivateResponse, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        let request = PrivateRequest {
            request_type: MessageText,
            username: self.name.clone(),
            file: vec![],
            message,
            filename: "".to_string(),
            trade_request: ["".to_string(), "".to_string()]
        };
        self.sender
            .send(Command::SendRequest {request, username, sender})
            .await
            .expect("Command not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }
    
    pub async fn offer_files(&mut self, filenames: Vec<String>, files: HashMap<String, Vec<u8>>) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::OfferFiles {filenames, sender, files})
            .await
            .expect("Command not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }
    
    pub async fn get_offerings(&mut self, username: String) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::GetOfferings { username, sender })
            .await
            .expect("Command not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }

    pub async fn offer_trade(&mut self, my_file: String, their_file: String, username: String) -> Result<PrivateResponse, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        let request = PrivateRequest {
            request_type: RequestResponseType::Trade,
            username: self.name.clone(),
            file: vec![],
            message: "".to_string(),
            filename: "".to_string(),
            trade_request: [my_file, their_file]
        };
        self.sender
            .send(Command::SendRequest {request, username, sender})
            .await
            .expect("Command not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }
    
    pub async fn offer_response(&mut self, response: bool) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::TradeResponse {response, sender})
            .await
            .expect("Command not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }

    pub async fn send_file(&mut self, username: String, filename: String) -> Result<PrivateResponse, Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::SendFile {username, filename, sender})
            .await
            .expect("Command not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }

    pub async fn connect_rendezvous(&mut self, server_address: String) -> Result<(), Box<dyn Error + Send>> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Command::ConnectRendezvous { server_address, sender })
            .await
            .expect("Command receiver not to be dropped");
        receiver.await.expect("Sender not to be dropped")
    }
}

pub async fn new(secret_key_seed: Option<u8>) -> Result<(Client, EventLoop), Box<dyn Error>> {
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
            request_response: request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/file-exchange/1"),
                    request_response::ProtocolSupport::Full
                    )],
                request_response::Config::default(),
            ),
            rendezvous: rendezvous::client::Behaviour::new(key.clone()),
        }))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    // Subscribing and picking an address before setting up kademlia works great

    // Create a Gossipsub topic
    let topic = gossipsub::IdentTopic::new("default");
    // subscribes to our topic
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    // Listen on all interfaces and whatever port the OS assigns
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));
    let (command_sender, command_receiver) = mpsc::channel(0);

    Ok((
        Client{
            sender: command_sender,
            peer_id,
            name: "".to_string()
        },
        EventLoop::new(swarm, command_receiver, )
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

struct TradeRequest {
        request: PrivateRequest,
        channel: ResponseChannel<PrivateResponse>
}

pub struct EventLoop {
    swarm: Swarm<MyBehaviour>,
    command_receiver: Receiver<Command>,
    topic: gossipsub::IdentTopic,
    registered_users: HashMap<PeerId, String>,
    rev_registered_users: HashMap<String, PeerId>,
    peer_addresses: PeerAddresses,
    pending_dial: HashMap<PeerId, oneshot::Sender<Result<(), Box<dyn Error + Send>>>>,
    pending_request: HashMap<OutboundRequestId, oneshot::Sender<Result<PrivateResponse, Box<dyn Error + Send>>>>,
    msg_log: HashMap<PeerId, Vec<String>>,
    my_files: HashMap<String, Vec<u8>>,
    awaiting_confirmation: Vec<TradeRequest>,
    last_discovery: std::time::Instant,
    rendezvous_server: Option<String>,
}

const FILES_NAMESPACE: &[u8] = b"/files/";
pub(crate) const OUTPUT_PATH: &str = "received/";

impl EventLoop {
    fn new(swarm: Swarm<MyBehaviour>, command_receiver: Receiver<Command>) -> Self {
        Self {
            swarm,
            command_receiver,
            topic: gossipsub::IdentTopic::new("default"),
            registered_users: HashMap::new(),
            rev_registered_users: HashMap::new(),
            pending_dial: Default::default(),
            pending_request: Default::default(),
            peer_addresses: PeerAddresses::new(NonZero::new(32usize).unwrap()),
            msg_log: Default::default(),
            my_files: Default::default(),
            awaiting_confirmation: Vec::new(),
            last_discovery: std::time::Instant::now(),
            rendezvous_server: None,
        }
    }

    pub async fn run(mut self) {
        let mut discovery_interval = tokio::time::interval(Duration::from_secs(60)); // Rediscover every 60 seconds

        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => self.handle_event(event).await,
                command = self.command_receiver.next() => match command {
                    Some(c) => self.handle_command(c).await,
                    None => return
                },
                _ = discovery_interval.tick() => {
                    // Periodically try to discover peers if we're connected to a rendezvous server
                    if let Some(server) = &self.rendezvous_server {
                        if let Ok(addr) = server.parse::<Multiaddr>() {
                            self.swarm.behaviour_mut().rendezvous.discover(
                                Some(Namespace::from_static(DEFAULT_NAMESPACE)),
                                None,
                                None,
                                PeerId::from_str(SERVER_PEER_ID).unwrap(),
                            )
                        }
                    }
                }
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<MyBehaviourEvent>) {
        match event {
            Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                for (peer_id, multiaddr) in list {
                    println!("mDNS discovered a new peer: {peer_id}");
                    self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                    self.peer_addresses.add(peer_id, multiaddr);
                }
            },
            Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                for (peer_id, multiaddr) in list {
                    println!("mDNS discover peer has expired: {peer_id}");
                    self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                    self.peer_addresses.remove(&peer_id, &multiaddr);
                }
            },
            Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: peer_id,
                message_id: _id,
                message})) => {
                if !self.registered_users.contains_key(&peer_id) {
                    let key = kad::RecordKey::new(&peer_id.to_bytes());
                    self.swarm.behaviour_mut().kademlia.get_record(key);
                    match self.msg_log.get_mut(&peer_id) {
                        None => {
                            let mut messages: Vec<String> = Vec::new();
                            let message_str = String::from_utf8_lossy(&message.data).to_string();
                            messages.push(message_str);
                            self.msg_log.insert(peer_id, messages);
                        }
                        Some(messages) => {
                            messages.push(String::from_utf8_lossy(&message.data).to_string());
                        }
                    }
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
                        let key_bytes = key.to_vec();

                        if key_bytes.starts_with(FILES_NAMESPACE) {
                            // Record is a username : list of files.
                            let username = match String::from_utf8(key_bytes[FILES_NAMESPACE.len()..].to_vec()) {
                                Ok(name) => {name}
                                Err(_) => {
                                    println!("Failed to decoded username");
                                    return
                                }
                            };
                            match serde_cbor::from_slice::<Vec<String>>(&*value) {
                                Ok(files) => {
                                    println!("Files offered by {username}:");
                                    for file in files {
                                        println!("{}", file);
                                    }
                                }
                                Err(_) => {
                                    println!("Failed to decoded files record");
                                    return
                                }
                            }

                        } else {
                            // Record is likely a peer_id : username, else error.
                            let peer_id = PeerId::from_bytes(key.as_ref()).unwrap();
                            let username = match String::from_utf8(value.clone()) {
                                Ok(name) => {name}
                                Err(e) => {
                                    println!("Failed to decode username with error {e}");
                                    return
                                }
                            };
                            self.registered_users.insert(peer_id, username.clone());
                            self.rev_registered_users.insert(username.clone(), peer_id);
                            match self.msg_log.get_mut(&peer_id) {
                                None => {}
                                Some(messages) => {
                                    for message in messages {
                                        println!("[Backlog] {username}: {}", message);
                                    }
                                }
                            }
                            self.msg_log.remove(&peer_id);
                        }
                    }
                    _ => {}
                }
            },
            SwarmEvent::NewListenAddr {address, ..} => {
                println!("Local node is listening on {address}")
            },
            Behaviour(MyBehaviourEvent::RequestResponse(
            request_response::Event::Message {message, ..},
                      )) => match message {
                Message::Request { request, channel, .. } => {
                    match request.request_type {
                        MessageText => {
                            println!("[Private Message] {}: {}", request.username, request.message);
                            let responder = PrivateResponse {
                                response_type: MessageText,
                                message: request.message,
                                file: Vec::new(),
                                filename: "".to_string(),
                                accepted: false
                            };
                            self
                                .swarm
                                .behaviour_mut()
                                .request_response
                                .send_response(channel, responder)
                                .expect("Connection to peer to still be open");
                        }
                        Trade => {
                            println!("Trade request from {}, they offer: {} for {}", request.username, request.trade_request.get(0).unwrap(), request.trade_request.get(1).unwrap());
                            println!("Type 'accept' or 'deny'");
                            self.awaiting_confirmation.push(TradeRequest {request, channel});
                        }
                        RequestResponseType::File => {
                            let file = request.file.clone();
                            let filename = request.filename.clone();
                            let path = OUTPUT_PATH.to_owned() + filename.as_str();
                            match fs::create_dir(OUTPUT_PATH) {
                                Ok(_) => {}
                                Err(_) => {}
                            }
                            match fs::write(path.clone(), file) {
                                Ok(_) => {println!("Saved file {} at {}", filename, path)}
                                Err(e) => {println!("Failed to write file, {}", e)}
                            }

                            let response = PrivateResponse {
                                response_type: RequestResponseType::File,
                                message: "".to_string(),
                                file: vec![],
                                filename: "".to_string(),
                                accepted: false,
                            };
                            self.swarm
                                .behaviour_mut()
                                .request_response
                                .send_response(channel, response)
                                .expect("Connection still to be open");
                        }
                    }
                }
                Message::Response { request_id, response} => {
                    let _ = self
                        .pending_request
                        .remove(&request_id)
                        .expect("Request to still be pending.")
                        .send(Ok(response));
                }
            },
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                if endpoint.is_dialer() {
                    if let Some(sender) = self.pending_dial.remove(&peer_id) {
                        let _ = sender.send(Ok(()));
                    }
                }
            },
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                if let Some(peer_id) = peer_id {
                    if let Some(sender) = self.pending_dial.remove(&peer_id) {
                        let _ = sender.send(Err(Box::new(error)));
                    }
                }
            },
            Behaviour(MyBehaviourEvent::Rendezvous(rendezvous::client::Event::Discovered { registrations, .. })) => {
                println!("Discovered {} peers via rendezvous", registrations.len());
                for registration in registrations {
                    let peer_id = registration.record.peer_id();
                    println!("Discovered peer: {}", peer_id);

                    self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);

                    if let Some(addr) = registration.record.addresses().first() {
                        println!("Address: {}", addr);
                        let _ = self.swarm.dial(addr.clone());
                        self.peer_addresses.add(peer_id, addr.clone());

                        if !self.registered_users.contains_key(&peer_id) {
                            let key = kad::RecordKey::new(&peer_id.to_bytes());
                            self.swarm.behaviour_mut().kademlia.get_record(key);
                        }
                    }
                }
            },
            Behaviour(MyBehaviourEvent::Rendezvous(rendezvous::client::Event::Registered { ttl, .. })) => {
                println!("Successfully registered with rendezvous server (TTL: {}s)", ttl);
            },
            Behaviour(MyBehaviourEvent::Rendezvous(rendezvous::client::Event::RegisterFailed { error, .. })) => {
                println!("Failed to register with rendezvous server: {:?}", error);
            },
            Behaviour(MyBehaviourEvent::Identify(identify::Event::Received {
                                                     peer_id, ..
                                                 })) => {
                println!("Identified peer: {}", peer_id);

                // If this is the rendezvous server and we're not yet registered, register now
                if let Some(_server_addr) = &self.rendezvous_server {
                    if peer_id == PeerId::from_str(SERVER_PEER_ID).unwrap() {
                        println!("Received identify from rendezvous server, attempting to register");

                        // Check if we now have external addresses
                        let external_addrs = self.swarm.external_addresses().collect::<Vec<_>>();
                        if !external_addrs.is_empty() {
                            println!("External addresses discovered: {:?}", external_addrs);

                            match self.swarm.behaviour_mut().rendezvous.register(
                                Namespace::from_static(DEFAULT_NAMESPACE),
                                peer_id,
                                None, // Use default TTL
                            ) {
                                Ok(_) => {
                                    println!("Successfully registered with rendezvous server after identify");
                                    // Initiate discovery
                                    self.swarm.behaviour_mut().rendezvous.discover(
                                        Some(Namespace::from_static(DEFAULT_NAMESPACE)),
                                        None,
                                        None,
                                        peer_id,
                                    );
                                },
                                Err(e) => println!("Failed to register with rendezvous server: {:?}", e),
                            }
                        } else {
                            println!("Still no external addresses available after identify");
                        }
                    }
                }
            },
            SwarmEvent::NewExternalAddrCandidate { address, .. } => {
                println!("Discovered external address: {}", address);
                self.swarm.add_external_address(address);

                // If we have a rendezvous server connection but aren't registered yet, try to register
                if let Some(_server_addr) = &self.rendezvous_server {
                    let server_peer_id = PeerId::from_str(SERVER_PEER_ID).unwrap();

                    match self.swarm.behaviour_mut().rendezvous.register(
                        Namespace::from_static(DEFAULT_NAMESPACE),
                        server_peer_id,
                        None, // Use default TTL
                    ) {
                        Ok(_) => {
                            println!("Successfully registered with rendezvous server after external address discovery");
                            // Initiate discovery
                            self.swarm.behaviour_mut().rendezvous.discover(
                                Some(Namespace::from_static(DEFAULT_NAMESPACE)),
                                None,
                                None,
                                server_peer_id,
                            );
                        },
                        Err(e) => println!("Failed to register with rendezvous server: {:?}", e),
                    }
                }
            },
            _ => {}
        }
    }



    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::SendMessage {message, sender} => {
                if !self.registered_users.contains_key(self.swarm.local_peer_id()) {
                    let error = CustomError {
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
                        self.registered_users.insert(user_info.peer_id, user_info.username.clone());
                        self.rev_registered_users.insert(user_info.username.clone(), user_info.peer_id);
                        sender.send(Ok(user_info.username)).expect("Sender failed");
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
            },
            Command::Dial {username, sender } => {
                let peer_id = match self.rev_registered_users.get(&username) {
                    None => { println!("Failed to identify peer"); return; }
                    Some(id) => {id}
                };
                let peer_add = self.peer_addresses.get(peer_id).next().unwrap();
                if let hash_map::Entry::Vacant(e) = self.pending_dial.entry(*peer_id) {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, peer_add.clone());
                    match self.swarm.dial(peer_add.with(Protocol::P2p(*peer_id))) {
                        Ok(()) => {
                            e.insert(sender);
                        }
                        Err(e) => {
                            let _ = sender.send(Err(Box::new(e)));
                        }
                    }
                } else {
                    println!("Already dialing peer");
                }
            },
            Command::SendRequest { request, username, sender } => {
                let peer = match self.rev_registered_users.get(&username) {
                    None => { 
                        println!("Failed to identify peer");
                        let error = CustomError {
                            message: "Peer not found".to_string()
                        };
                        sender.send(Err(Box::new(error))).expect("Sender failed");
                        return
                    }
                    Some(id) => {id}
                };
                let request_id = self.
                    swarm.
                    behaviour_mut().
                    request_response.
                    send_request(&peer, request.clone());
                self.pending_request.insert(request_id.clone(), sender);
            },
            Command::SendFile {username, filename, sender} => {
                let peer = match self.rev_registered_users.get(&username) {
                    None => {
                        println!("Failed to identify peer");
                        let error = CustomError {
                            message: "Peer not found".to_string()
                        };
                        sender.send(Err(Box::new(error))).expect("Sender failed");
                        return
                    }
                    Some(id) => {id}
                };
                let file = self.my_files.get(&filename).unwrap();
                let request = PrivateRequest {
                    request_type: RequestResponseType::File,
                    file: (*file.clone()).to_owned(),
                    username,
                    message: "".to_string(),
                    filename,
                    trade_request: ["".to_string(), "".to_string()],
                };
                let request_id = self.
                    swarm.
                    behaviour_mut().
                    request_response.
                    send_request(&peer, request);
                self.pending_request.insert(request_id.clone(), sender);
            }
            Command::OfferFiles { filenames, sender, files } => {
                let mut record_key = FILES_NAMESPACE.to_vec();
                let username = self.registered_users.get(self.swarm.local_peer_id()).unwrap();
                record_key.append(&mut username.clone().into_bytes());
                let record_value = match serde_cbor::to_vec(&filenames) {
                    Ok(file_bytes) => {file_bytes}
                    Err(_) => {
                        println!("Failed to serialize files");
                        return
                    }
                };
                let record = kad::Record {
                    key: kad::RecordKey::new(&record_key),
                    value: record_value,
                    publisher: None,
                    expires: None
                };
                match self.swarm.behaviour_mut().kademlia.put_record(record, Quorum::One) {
                    Ok(_) => {
                        println!("Successfully offered files");
                        self.my_files = files;
                        sender.send(Ok(())).expect("Sender failed");
                    }
                    Err(e) => {
                        println!("Failed to offer files");
                        sender.send(Err(Box::new(e))).expect("Sender failed");
                    }
                }
            },
            Command::GetOfferings {username, sender} => {
                let mut record_key = FILES_NAMESPACE.to_vec();
                record_key.append(&mut username.clone().into_bytes());
                let key = kad::RecordKey::new(&record_key);
                self.swarm.behaviour_mut().kademlia.get_record(key);
                sender.send(Ok(())).expect("Sender failed");
            },
            Command::TradeResponse {response, sender} => {
                let trade_request = self.awaiting_confirmation.pop().unwrap();
                match response {
                    true => {
                        let filename = trade_request.request.trade_request.get(1).unwrap();
                        let offered_file = trade_request.request.trade_request.get(0).unwrap();
                        let file = self.my_files.get(filename).unwrap();
                        let response = PrivateResponse {
                            response_type: Trade,
                            message: offered_file.to_string(),
                            file: (*file.clone()).to_owned(),
                            filename: filename.clone(),
                            accepted: true,
                        };
                        println!("Trade accepted, sending file {}", filename);
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .send_response(trade_request.channel, response)
                            .expect("Channel still to be open");
                    }
                    false => {
                        let response = PrivateResponse {
                            response_type: Trade,
                            message: "".to_string(),
                            file: vec![],
                            filename: "".to_string(),
                            accepted: false,
                        };
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .send_response(trade_request.channel, response)
                            .expect("Channel still to be open");
                        println!("Trade request has been rejected");
                    },
                }
                sender.send(Ok(())).expect("Sender failed");
            },

            // AI help to integrate the rendezvous server
            Command::ConnectRendezvous { server_address, sender } => {
                println!("Connecting to rendezvous server at: {}", server_address);
                match server_address.parse::<Multiaddr>(){
                    Ok(addr) => {
                        match self.swarm.dial(addr.clone()) {
                            Ok(_) => {
                                println!("Successfully dialed rendezvous server");

                                // Store the server address for later use
                                self.rendezvous_server = Some(server_address.clone());

                                // We'll wait for the identify protocol to discover our external addresses
                                // before attempting to register with the rendezvous server

                                // Check if we already have external addresses
                                let has_external_addresses = !self.swarm.external_addresses().collect::<Vec<_>>().is_empty();

                                if has_external_addresses {
                                    // We already have external addresses, so we can register immediately
                                    match self.swarm.behaviour_mut().rendezvous.register(
                                        Namespace::from_static(DEFAULT_NAMESPACE),
                                        PeerId::from_str(SERVER_PEER_ID).unwrap(),
                                        None // Use default TTL
                                    ) {
                                        Ok(_) => {
                                            println!("Registered with rendezvous server");
                                            self.swarm.behaviour_mut().rendezvous.discover(
                                                Some(Namespace::from_static(DEFAULT_NAMESPACE)),
                                                None,
                                                None,
                                                PeerId::from_str(SERVER_PEER_ID).unwrap(),
                                            );
                                            sender.send(Ok(())).expect("Sender failed");
                                        },
                                        Err(e) => {
                                            println!("Failed to register with rendezvous server: {:?}", e);
                                            sender.send(Err(Box::new(e))).expect("Sender failed");
                                        }
                                    }
                                } else {
                                    // No external addresses yet, we'll defer the registration
                                    // The registration will be triggered by the Identify event
                                    println!("Waiting for external address discovery before registering...");
                                    sender.send(Ok(())).expect("Sender failed");
                                }
                            },
                            Err(e) => {
                                println!("Failed to dial rendezvous server: {:?}", e);
                                sender.send(Err(Box::new(e))).expect("Sender failed");
                            }
                        }
                    },
                    Err(e) => {
                        println!("Invalid rendezvous server address: {:?}", e);
                        sender.send(Err(Box::new(CustomError {
                            message: format!("Invalid address: {}", e)
                        }))).expect("Sender failed");
                    }
                }
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum RequestResponseType {
    MessageText,
    File,
    Trade
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PrivateRequest {
    request_type: RequestResponseType,
    file: Vec<u8>,
    username: String,
    message: String,
    filename: String,
    trade_request: [String; 2]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateResponse {
    pub(crate) response_type: RequestResponseType,
    pub message: String,
    pub file: Vec<u8>,
    pub filename: String,
    pub accepted: bool
}

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    mdns: mdns::tokio::Behaviour,
    identify: identify::Behaviour,
    request_response: request_response::cbor::Behaviour<PrivateRequest, PrivateResponse>,
    rendezvous: rendezvous::client::Behaviour,
}