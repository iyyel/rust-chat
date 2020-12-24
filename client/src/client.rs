use futures::{future, pin_mut, StreamExt};

use std::collections::HashSet;

use async_std::io;
use serde::{Deserialize, Serialize};

use async_std::prelude::*;
use async_std::task;
use async_tungstenite::async_std::connect_async;
use async_tungstenite::tungstenite::protocol::Message as TungMessage;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    src_addr: String,
    src_name: String,
    msg_type: MessageType,
    msg: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum MessageType {
    NewPeer(String), // Broadcast this message to all peers when a new peer has connected. The parameter is the name of the peer that has connected.
    LostPeer(String), // Broadcast this message to all peers when a peers has disconnected. The parameter is the name of the peer that has disconnected.
    Text,             // Standard peer text message (broadcasted message)
    PeerDataRequest,
    PeerDataReply(PeerData), // If received by the server, a peer is asking to retrieve data about all connected peers. If received by a peer, it is the data from the server.
    PeerName(String), // The server sends this message to a client when it has first connected, giving it a random name. The name is the parameter.
    Private(String),  // A private message to the given name of a peer.
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct PeerData {
    count: i32,
    peer_names: HashSet<String>
}

pub struct Client {
    addr: String,
    name: String,
}

impl Client {
    pub fn new(addr: String) -> Self {
        Self {
            addr,
            name: String::new(),
        }
    }

    pub async fn connect(&mut self) {
        let (sender, receiver) = futures::channel::mpsc::unbounded::<TungMessage>();

        let (ws_stream, _) = connect_async(format!("ws://{}/socket", &self.addr))
            .await
            .expect("Failed to connect");

        println!("WebSocket handshake has been successfully completed.");

        let local_addr = ws_stream.get_ref().local_addr().unwrap().to_string();

        let (write, mut read) = ws_stream.split();

        let stdin_to_ws = receiver.map(Ok).forward(write);

        // Wait until name message has been received.
        loop {
            if let Some(msg) = read.next().await {
                let msg = msg.unwrap().to_string();
                let msg: Message = serde_json::from_str(&msg).unwrap();
                let msg_type = msg.msg_type.clone();

                match msg_type {
                    MessageType::PeerName(new_name) => {
                        async_std::io::stdout()
                            .write_all(format!("\nWelcome to Rust-Chat, {}!", new_name).as_bytes())
                            .await
                            .unwrap();
                        self.name = new_name;
                        async_std::io::stdout().flush().await.unwrap();
                        break;
                    }
                    _ => continue,
                }
            };
        }

        task::spawn(read_stdin(sender, local_addr, self.name.clone()));

        let ws_to_stdout = async {
            while let Some(msg) = read.next().await {
                let msg = msg.unwrap().to_string();
                let msg: Message = serde_json::from_str(&msg).unwrap();
                let msg_type = msg.msg_type.clone();

                match msg_type {
                    MessageType::NewPeer(peer_name) => async_std::io::stdout()
                        .write_all(
                            format!("\n{}: {} has connected.", &msg.src_name, peer_name).as_bytes(),
                        )
                        .await
                        .unwrap(),
                    MessageType::LostPeer(peer_name) => async_std::io::stdout()
                        .write_all(
                            format!("\n{}: {} has disconnected.", &msg.src_name, peer_name)
                                .as_bytes(),
                        )
                        .await
                        .unwrap(),
                    MessageType::Text => async_std::io::stdout()
                        .write_all(format!("\n{}: {}", &msg.src_name, &msg.msg).as_bytes())
                        .await
                        .unwrap(),
                    MessageType::PeerDataRequest => async_std::io::stdout()
                        .write_all(format!("\n{}: {}", &msg.src_name, &msg.msg).as_bytes())
                        .await
                        .unwrap(),
                    MessageType::PeerDataReply(peer_data) => async_std::io::stdout()
                        .write_all(format!("\n{}: {:?}", &msg.src_name, peer_data).as_bytes())
                        .await
                        .unwrap(),
                    MessageType::PeerName(name) => {
                        async_std::io::stdout()
                            .write_all(
                                format!("\n{}: {}, {}", &msg.src_name, &msg.msg, name).as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    MessageType::Private(name) => async_std::io::stdout()
                        .write_all(
                            format!("\n{}: {}: {}", &msg.src_name, &msg.msg, name).as_bytes(),
                        )
                        .await
                        .unwrap(),
                }
                async_std::io::stdout().flush().await.unwrap();
            }
        };

        pin_mut!(stdin_to_ws, ws_to_stdout);
        future::select(stdin_to_ws, ws_to_stdout).await;
    }
}

// Our helper method which will read data from stdin and send it along the
// sender provided.
async fn read_stdin(
    sender: futures::channel::mpsc::UnboundedSender<TungMessage>,
    local_addr: String,
    peer_name: String,
) {
    let mut stdin = io::stdin();

    loop {
        async_std::io::stdout()
            .write_all(format!("\n{}: ", peer_name).as_bytes())
            .await
            .unwrap();
        async_std::io::stdout().flush().await.unwrap();

        let mut buf = vec![0; 1024];
        let n = match stdin.read(&mut buf).await {
            Err(_) | Ok(0) => break,
            Ok(n) => n,
        };
        buf.truncate(n);

        let msg = String::from_utf8(buf).unwrap();

        if msg.starts_with("pm: ") {
            let split: Vec<&str> = msg.split(" ").collect();
            let (recv_name, msg) = (split[1].to_string(), split[2].to_string());

            let msg_struct = Message {
                src_addr: local_addr.to_string(),
                src_name: peer_name.to_string(),
                msg_type: MessageType::Private(recv_name),
                msg: msg,
            };

            sender
                .unbounded_send(TungMessage::Text(
                    serde_json::to_string(&msg_struct).unwrap(),
                ))
                .unwrap();
        } else if msg.starts_with("peerdata: ") {
            let msg_struct = Message {
                src_addr: local_addr.to_string(),
                src_name: peer_name.to_string(),
                msg_type: MessageType::PeerDataRequest,
                msg: String::from(""),
            };

            sender
                .unbounded_send(TungMessage::Text(
                    serde_json::to_string(&msg_struct).unwrap(),
                ))
                .unwrap();
        
        } else {

        let msg_struct = Message {
            src_addr: local_addr.to_string(),
            src_name: peer_name.to_string(),
            msg_type: MessageType::Text,
            msg: msg,
        };

        sender
            .unbounded_send(TungMessage::Text(
                serde_json::to_string(&msg_struct).unwrap(),
            ))
            .unwrap();
        }
    }
}
