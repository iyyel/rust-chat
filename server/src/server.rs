use rand::seq::SliceRandom;
use std::collections::HashMap;
use std::io::Error as IoError;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use futures::channel::mpsc::{unbounded, UnboundedSender};
use futures::prelude::*;
use futures::{future, pin_mut};

use async_std::net::{TcpListener, TcpStream};
use async_std::task;
use async_tungstenite::tungstenite::protocol::Message as TungMessage;

type Sender = UnboundedSender<TungMessage>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Sender>>>;
type PeerNameMap = Arc<Mutex<HashMap<SocketAddr, String>>>;

const NAME: &str = "Server";

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    src_addr: String,
    name: String,
    msg_type: MessageType,
    msg: String,
}

// NOTES:
//
// Try to see if you can avoid using Ip's for communication to the peers.
// If the peers only use communication with names, it is more secure and anonymous.
//

#[derive(Serialize, Deserialize, Debug, Clone)]
enum MessageType {
    NewPeer(String), // Broadcast this message to all peers when a new peer has connected. The parameter is the name of the peer that has connected.
    LostPeer(String), // Broadcast this message to all peers when a peers has disconnected. The parameter is the name of the peer that has disconnected.
    Text,             // Standard peer text message (broadcasted message)
    PeerData, // If received by the server, a peer is asking to retrieve data about all connected peers. If received by a peer, it is the data from the server.
    PeerName(String), // The server sends this message to a client when it has first connected, giving it a random name. The name is the parameter.
    Private(String),  // A private message to the given name of a peer.
}

pub struct Server {
    addr: String,
    peers: PeerMap,
    peer_names: PeerNameMap,
}

impl Server {
    pub fn new(addr: String) -> Self {
        Self {
            addr,
            peers: PeerMap::new(Mutex::new(HashMap::new())),
            peer_names: PeerNameMap::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(&self) -> Result<(), IoError> {
        // Create the event loop and TCP listener we'll accept connections on.
        let try_socket = TcpListener::bind(&self.addr).await;
        let listener = try_socket.expect("Failed to bind");
        println!("Listening on: {}", &self.addr);

        // Let's spawn the handling of each connection in a separate task.
        while let Ok((stream, peer_addr)) = listener.accept().await {
            task::spawn(on_peer_connect(
                self.peers.clone(),
                self.peer_names.clone(),
                stream,
                self.addr.clone(),
                peer_addr,
            ));
        }

        Ok(())
    }
}

async fn on_peer_connect(
    peers: PeerMap,
    peer_names: PeerNameMap,
    raw_stream: TcpStream,
    local_addr: String,
    peer_addr: SocketAddr,
) {
    println!("\nIncoming TCP connection from: {}", peer_addr);

    let ws_stream = async_tungstenite::accept_async(raw_stream)
        .await
        .expect("Error during the websocket handshake occurred");

    let mut names = vec!["Charles", "James"];
    let peer_name = names.choose(&mut rand::thread_rng()).unwrap().to_string();
    names.remove(names.iter().position(|x| *x == peer_name).unwrap());

    // bind the peer address to the given name such that we can remove it
    // later, but not re-use it while the peer is still active.
    peer_names
        .lock()
        .unwrap()
        .insert(peer_addr, peer_name.clone());

    broadcast_new_peer_msg(&peers, &local_addr, &peer_addr, &peer_name);
    println!("{} ({}) has connected.", peer_name, peer_addr);

    let (sender, receiver) = unbounded();
    send_name_msg(&sender, &peer_name, &local_addr);

    // Insert the write part of this peer to the peer map.
    peers.lock().unwrap().insert(peer_addr, sender);

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
                MessageType::Text => {
                    println!("\n{}: {:?}", peer_addr, msg);
                    broadcast_msg(&peers, &peer_addr, msg);
                }
                MessageType::PeerData => {
                    println!("\nSending client data: {}: {:?}", peer_addr, msg);
                }
                MessageType::Private(addr) => {
                    let socket: SocketAddr = addr.clone().parse().unwrap();
                    send_msg(&peers, &socket, msg);
                }
                _ => (),
            }

            future::ok(())
        });

    let receive_from_others = receiver.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    let peer_names_lock = peer_names.lock().unwrap();
    let lost_name = peer_names_lock.get(&peer_addr).unwrap();

    broadcast_lost_peer_msg(&peers, &local_addr, &peer_addr, lost_name);
    println!("{} ({}) has disconnected.", peer_name, peer_addr);

    names.push(lost_name);
    peer_names.lock().unwrap().remove(&peer_addr);
    peers.lock().unwrap().remove(&peer_addr);
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

fn send_msg(peers: &PeerMap, peer_addr: &SocketAddr, msg: Message) {
    let peers = peers.lock().unwrap();
    let msg = TungMessage::Text(serde_json::to_string(&msg).unwrap());

    // Send only to single peer!
    let recp = peers.get(&peer_addr).unwrap();
    recp.unbounded_send(msg.clone()).unwrap();
}

fn broadcast_new_peer_msg(
    peers: &PeerMap,
    local_addr: &String,
    peer_addr: &SocketAddr,
    peer_name: &String,
) {
    let msg = Message {
        src_addr: local_addr.clone(),
        name: NAME.to_string(),
        msg_type: MessageType::NewPeer(peer_name.clone()),
        msg: format!("{} ({}) has connected.", peer_name, peer_addr),
    };

    broadcast_msg(peers, peer_addr, msg);
}

fn broadcast_lost_peer_msg(
    peers: &PeerMap,
    local_addr: &String,
    peer_addr: &SocketAddr,
    peer_name: &String,
) {
    let msg = Message {
        src_addr: local_addr.clone(),
        name: NAME.to_string(),
        msg_type: MessageType::LostPeer(peer_name.clone()),
        msg: format!("{} ({}) has disconnected.", peer_name, peer_addr),
    };

    broadcast_msg(peers, peer_addr, msg);
}

fn send_name_msg(sender: &Sender, peer_name: &String, local_addr: &String) {
    let msg = Message {
        src_addr: local_addr.clone(),
        name: NAME.to_string(),
        msg_type: MessageType::PeerName(peer_name.clone()),
        msg: "Name message".to_string(),
    };

    let msg = TungMessage::Text(serde_json::to_string(&msg).unwrap());
    sender.unbounded_send(msg).unwrap();
}
