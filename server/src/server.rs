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
type PeerData = (String, Sender);
type PeerMap = Arc<Mutex<HashMap<SocketAddr, PeerData>>>;

// Add new message types:
// Broadcast, PM
// New Client, Disconnect
// AssignName (MAYBE?)
// OnlineClient Names?
// Online client amount?

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    src_addr: String,
    name: String,
    msg_type: MessageType,
    msg: String,
}

#[derive(Serialize, Deserialize, Debug)]
enum MessageType {
    Broadcast,
    ChangeName(String)
}

pub struct Server {
    addr: String,
    peers: PeerMap,
}

const NAME: &str = "Server";

impl Server {
    pub fn new(addr: String) -> Self {
        Self {
            addr,
            peers: PeerMap::new(Mutex::new(HashMap::new())),
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
    raw_stream: TcpStream,
    local_addr: String,
    peer_addr: SocketAddr,
) {
    println!("\nIncoming TCP connection from: {}", peer_addr);

    let ws_stream = async_tungstenite::accept_async(raw_stream)
        .await
        .expect("Error during the websocket handshake occurred");

    broadcast_new_client_msg(&peers, &local_addr, &peer_addr);
    println!("{} has connected.", peer_addr);

    // Insert the write part of this peer to the peer map.
    let (sender, receiver) = unbounded();
    peers.lock().unwrap().insert(peer_addr, ("no_name".to_string(), sender));

    let (outgoing, incoming) = ws_stream.split();




    let broadcast_incoming = incoming
        .try_filter(|msg| {
            // Broadcasting a Close message from one client
            // will close the other clients.
            future::ready(!msg.is_close())
        })
        .try_for_each(|msg| {
            let msg: Message = serde_json::from_str(msg.to_text().unwrap()).unwrap();

            match msg.msg_type {
                MessageType::Broadcast => {
                    println!("\n{}: {:?}", peer_addr, msg);
                    broadcast_msg(&peers, &peer_addr, msg);
                },
                MessageType::ChangeName(new_name) => {
                    println!("wow, {}", new_name);
                    let (old_name, s) = peers.lock().unwrap().get(&peer_addr).unwrap().clone();
                    peers.lock().unwrap().insert(peer_addr, (new_name.clone(), s.clone()));

                    let msg = Message {
                        src_addr: local_addr.clone(),
                        name: NAME.to_string(),
                        msg_type: MessageType::Broadcast,
                        msg: format!("{} is now {}", old_name, new_name),
                    };

                    broadcast_msg(&peers, &peer_addr, msg);
                }
            }

            future::ok(())
        });

    let receive_from_others = receiver.map(Ok).forward(outgoing);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    broadcast_discon_msg(&peers, &local_addr, &peer_addr);
    println!("{} has disconnected.", peer_addr);
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

    for (_, recp) in broadcast_recipients {
        println!("{:?} hmm", recp);
        recp.unbounded_send(msg.clone()).unwrap();
    }
}

fn broadcast_new_client_msg(peers: &PeerMap, local_addr: &String, peer_addr: &SocketAddr) {
    let msg = Message {
        src_addr: local_addr.clone(),
        name: NAME.to_string(),
        msg_type: MessageType::Broadcast,
        msg: format!("{} has connected.", peer_addr),
    };

    broadcast_msg(peers, peer_addr, msg);
}

fn broadcast_discon_msg(peers: &PeerMap, local_addr: &String, peer_addr: &SocketAddr) {
    let msg = Message {
        src_addr: local_addr.clone(),
        name: NAME.to_string(),
        msg_type: MessageType::Broadcast,
        msg: format!("{} has disconnected.", peer_addr),
    };

    broadcast_msg(peers, peer_addr, msg);
}