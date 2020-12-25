use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Error as IoError},
    iter::FromIterator,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use async_std::{
    net::{TcpListener, TcpStream},
    task,
};

use futures::{
    channel::mpsc::{unbounded, UnboundedSender},
    future, pin_mut,
    prelude::*,
};

use async_tungstenite::tungstenite::protocol::Message as TungMessage;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

type Sender = UnboundedSender<TungMessage>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Sender>>>;
type PeerNameMap = Arc<Mutex<HashMap<String, SocketAddr>>>;

const LOCAL_NAME: &str = "Server";

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message<'a> {
    src_name: &'a str,
    src_addr: &'a str,
    msg_type: MessageType<'a>,
    text: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum MessageType<'a> {
    NewPeer(&'a str), // Broadcast this message to all peers when a new peer has connected. The parameter is the name of the new peer that has connected.
    DisconPeer(&'a str), // Broadcast this message to all peers when a peer has disconnected. The parameter is the name of the peer that has disconnected.
    PeerNameAssign(&'a str), // The server sends this message to a peer when it has first connected, giving it a random name. The name is the parameter.
    PeerInfoRequest, // A peer sends this message to the server if the peer wants to retrieve peer info (PeerDataReply message is sent back to the peer).
    PeerInfoReply(PeerInfo), // If the server has received a PeerDataRequest message, a peer is asking to retrieve data about all connected peers. This resides in the PeerInfo struct.
    Private(&'a str), // A private message to the given peer. The parameter is the name of the peer receiving the message.
    Text,             // Standard broadcasted text message to all peers.
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct PeerInfo {
    peers_online: i32,           // How many peers are currently online?
    peer_spots_left: i32,        // How many available spots are left for connections?
    peer_names: HashSet<String>, // What are the names of the connected peers? excluding the requesting peers name.
}

pub struct Server {
    addr: String,
    peer_map: PeerMap,
    peer_name_map: PeerNameMap,
}

impl Server {
    pub fn new(addr: String) -> Self {
        Self {
            addr,
            peer_map: PeerMap::new(Mutex::new(HashMap::new())),
            peer_name_map: PeerNameMap::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(&self) -> Result<(), IoError> {
        // Create the event loop and TCP listener we'll accept connections on.
        let try_socket = TcpListener::bind(&self.addr).await;
        let listener = try_socket.expect("Failed to bind");
        println!("Listening on: {}", &self.addr);

        let names = parse_peer_names();

        // Let's spawn the handling of each connection in a separate task.
        while let Ok((stream, peer_addr)) = listener.accept().await {
            task::spawn(on_peer_connect(
                self.peer_map.clone(),
                self.peer_name_map.clone(),
                stream,
                self.addr.clone(),
                peer_addr,
                names.clone(),
            ));
        }

        Ok(())
    }
}

async fn on_peer_connect(
    peer_map: PeerMap,
    peer_name_map: PeerNameMap,
    raw_stream: TcpStream,
    local_addr: String,
    peer_addr: SocketAddr,
    names: HashSet<String>,
) {
    println!("\nIncoming TCP connection from: {}", peer_addr);
    let local_addr = local_addr.as_str();

    let ws_stream = async_tungstenite::accept_async(raw_stream)
        .await
        .expect("Error during the websocket handshake occurred");

    let available_peer_names = available_peer_names(&peer_name_map, &names);
    let peer_name = random_peer_name(&available_peer_names);
    let peer_spots_left: i32 = (available_peer_names.len() - 1) as i32;

    if peer_spots_left < 0 {
        println!("NO ROOM FOR MORE PEERS!");
        return;
    }

    // bind the peer name to the given peer address such that we can remove it
    // later, but not re-use the name while the peer is still active.
    peer_name_map
        .lock()
        .unwrap()
        .insert(peer_name.to_string(), peer_addr);

    broadcast_new_peer_msg(&peer_map, &local_addr, &peer_addr, &peer_name);
    println!("{} ({}) has connected.", peer_name, peer_addr);
    println!("Peer spots left: {}", peer_spots_left);

    let (sender, receiver) = unbounded();

    // assign the new peer the name of 'peer_name'
    send_name_assignment_msg(&sender, &local_addr, &peer_name);

    // Insert the write part of this peer to the peer map.
    peer_map.lock().unwrap().insert(peer_addr, sender);

    let (outgoing, incoming) = ws_stream.split();

    let broadcast_incoming = incoming
        .try_filter(|msg| {
            // Broadcasting a Close message from one client
            // will close the other clients.
            future::ready(!msg.is_close())
        })
        .try_for_each(|msg| {
            let msg: Message = serde_json::from_str(msg.to_text().unwrap()).unwrap();
            let msg_type = msg.msg_type.clone();

            match msg_type {
                MessageType::Text => handle_text_msg(&peer_map, &peer_addr, msg),
                MessageType::PeerInfoRequest => handle_peer_info_request_msg(
                    &peer_map,
                    &peer_name_map,
                    &names,
                    local_addr,
                    &peer_name,
                    &peer_addr,
                    msg,
                ),
                MessageType::PeerInfoReply(peer_info) => {
                    handle_peer_info_reply_msg(&peer_addr, peer_info, msg)
                }
                MessageType::Private(recv_peer_name) => handle_private_msg(
                    &peer_map,
                    &peer_name_map,
                    recv_peer_name,
                    &peer_name,
                    &peer_addr,
                    local_addr,
                    msg,
                ),
                _ => handle_unknown_msg(&peer_addr, msg),
            }

            future::ok(())
        });

    let receive_from_others = receiver.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    let discon_peer_name = discon_peer_name(&peer_name_map, &peer_addr).unwrap();
    peer_name_map.lock().unwrap().remove(&discon_peer_name);
    peer_map.lock().unwrap().remove(&peer_addr);

    broadcast_lost_peer_msg(&peer_map, &local_addr, &peer_addr, &discon_peer_name);
    println!("\n[Chat] {} ({}) has disconnected.", peer_name, peer_addr);
}

fn broadcast_msg(peers: &PeerMap, peer_addr: &SocketAddr, msg: Message) {
    let peers = peers.lock().unwrap();
    let msg = TungMessage::Text(serde_json::to_string(&msg).unwrap());

    // We want to broadcast the message to everyone except ourselves.
    let broadcast_recipients = peers
        .iter()
        .filter(|(addr, _)| addr != &peer_addr)
        .map(|(_, ws_sink)| ws_sink);

    for recp in broadcast_recipients {
        recp.unbounded_send(msg.clone()).unwrap();
    }
}

fn send_single_msg(peers: &PeerMap, peer_addr: &SocketAddr, msg: Message) {
    let peers = peers.lock().unwrap();
    let msg = TungMessage::Text(serde_json::to_string(&msg).unwrap());

    // Send only to single peer!
    let recp = peers.get(&peer_addr).unwrap();
    recp.unbounded_send(msg.clone()).unwrap();
}

fn broadcast_new_peer_msg(
    peers: &PeerMap,
    local_addr: &str,
    peer_addr: &SocketAddr,
    peer_name: &str,
) {
    let msg = Message {
        src_addr: local_addr,
        src_name: LOCAL_NAME,
        msg_type: MessageType::NewPeer(peer_name),
        text: format!("{} ({}) has connected.", peer_name, peer_addr),
    };

    broadcast_msg(peers, peer_addr, msg);
}

fn broadcast_lost_peer_msg(
    peers: &PeerMap,
    local_addr: &str,
    peer_addr: &SocketAddr,
    peer_name: &str,
) {
    let msg = Message {
        src_addr: local_addr,
        src_name: LOCAL_NAME,
        msg_type: MessageType::DisconPeer(peer_name.clone()),
        text: format!("{} ({}) has disconnected.", peer_name, peer_addr),
    };

    broadcast_msg(peers, peer_addr, msg);
}

fn send_name_assignment_msg(sender: &Sender, local_addr: &str, peer_name: &str) {
    let msg = Message {
        src_addr: local_addr,
        src_name: LOCAL_NAME,
        msg_type: MessageType::PeerNameAssign(peer_name.clone()),
        text: String::from("PeerName"),
    };

    let msg = TungMessage::Text(serde_json::to_string(&msg).unwrap());
    sender.unbounded_send(msg).unwrap();
}

fn create_peer_data(
    peer_name_map: &PeerNameMap,
    names: &HashSet<String>,
    src_name: &str,
) -> PeerInfo {
    let peer_names: HashSet<String> = peer_name_map
        .lock()
        .unwrap()
        .keys()
        .filter(|&k| k != src_name)
        .map(|k| k.to_string())
        .collect();

    let name_map = peer_name_map.lock().unwrap().clone();
    let curr_names: HashSet<String> = name_map.keys().map(|k| k.to_string()).collect();
    let available_names: Vec<String> = names
        .difference(&curr_names)
        .map(|n| n.to_string())
        .collect();

    let peer_spots_left = available_names.len() as i32;

    let peers_online = peer_name_map.lock().unwrap().keys().len() as i32;

    let peer_data = PeerInfo {
        peers_online: peers_online,
        peer_spots_left: peer_spots_left,
        peer_names: peer_names.clone(),
    };

    peer_data
}

fn parse_peer_names() -> HashSet<String> {
    let mut file = File::open("names.txt").expect("file error");
    let reader = BufReader::new(&mut file);

    let mut lines: Vec<_> = reader
        .lines()
        .map(|l| l.expect("Couldn't read a line"))
        .collect();

    lines.sort();
    lines.dedup();

    HashSet::from_iter(lines)
}

fn available_peer_names(peer_name_map: &PeerNameMap, names: &HashSet<String>) -> Vec<String> {
    let name_map = peer_name_map.lock().unwrap().clone();
    let curr_names: HashSet<String> = name_map.keys().map(|k| k.to_string()).collect();

    names
        .difference(&curr_names)
        .map(|n| n.to_string())
        .collect()
}

fn random_peer_name(available_peer_names: &[String]) -> String {
    available_peer_names
        .choose(&mut rand::thread_rng())
        .unwrap()
        .to_string()
}

fn discon_peer_name(peer_name_map: &PeerNameMap, discon_peer_addr: &SocketAddr) -> Option<String> {
    let peer_names = peer_name_map.lock().unwrap();

    for key in peer_names.keys() {
        if peer_names.get(key).unwrap() == discon_peer_addr {
            return Some(key.clone());
        }
    }

    None
}

fn handle_text_msg(peer_map: &PeerMap, peer_addr: &SocketAddr, msg: Message) {
    if !msg.text.trim().is_empty() {
        println!("\n[Chat] {} ({}): {}", msg.src_name, peer_addr, msg.text);
        broadcast_msg(&peer_map, &peer_addr, msg);
    }
}

fn handle_peer_info_request_msg(
    peer_map: &PeerMap,
    peer_name_map: &PeerNameMap,
    names: &HashSet<String>,
    local_addr: &str,
    peer_name: &String,
    peer_addr: &SocketAddr,
    msg: Message,
) {
    let peer_data = create_peer_data(&peer_name_map, &names, &msg.src_name);

    let msg = Message {
        src_addr: local_addr,
        src_name: LOCAL_NAME,
        msg_type: MessageType::PeerInfoReply(peer_data.clone()),
        text: String::from(""),
    };

    println!(
        "\n[PeerDataRequest] {} ({}) -> {} ({}): {:?}",
        LOCAL_NAME, local_addr, peer_name, peer_addr, peer_data
    );

    send_single_msg(&peer_map, &peer_addr, msg);
}

fn handle_peer_info_reply_msg(peer_addr: &SocketAddr, peer_info: PeerInfo, msg: Message) {
    println!(
        "\n[PeerDataReply] {} ({}): Server received PeerDataReply. Not further action is taken:
    {}: {:?}",
        msg.src_name, peer_addr, msg.text, peer_info
    )
}

fn handle_private_msg(
    peer_map: &PeerMap,
    peer_name_map: &PeerNameMap,
    recv_peer_name: &str,
    peer_name: &String,
    peer_addr: &SocketAddr,
    local_addr: &str,
    msg: Message,
) {
    if !msg.text.trim().is_empty() {
        if let Some(recv_peer_addr) = &peer_name_map
            .lock()
            .unwrap()
            .get(&recv_peer_name.to_string())
        {
            if recv_peer_name != peer_name {
                println!(
                    "\n[PM] {} ({}) -> {} ({}): {}",
                    msg.src_name, peer_addr, recv_peer_name, recv_peer_addr, msg.text
                );

                send_single_msg(&peer_map, recv_peer_addr, msg);
            }
        } else {
            let msg = Message {
                src_addr: local_addr,
                src_name: LOCAL_NAME,
                msg_type: MessageType::Private(peer_name.as_str()),
                text: format!("{} is not connected.", recv_peer_name),
            };

            println!(
                "\n[PM] {} ({}) -> {} ({}): {}",
                LOCAL_NAME, local_addr, peer_name, peer_addr, &msg.text
            );

            send_single_msg(&peer_map, &peer_addr, msg);
        }
    }
}

fn handle_unknown_msg(peer_addr: &SocketAddr, msg: Message) {
    println!(
        "\n[Chat: UNKNOWN MESSAGE] {} ({}): {}",
        msg.src_name, peer_addr, msg.text
    )
}
