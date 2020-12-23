use futures::{future, pin_mut, StreamExt};

use async_std::io;
use serde::{Deserialize, Serialize};

use async_std::prelude::*;
use async_std::task;
use async_tungstenite::async_std::connect_async;
use async_tungstenite::tungstenite::protocol::Message as TungMessage;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    src_addr: String,
    name: String,
    msg_type: MessageType,
    msg: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum MessageType {
    NewPeer(String), // Broadcast this message to all peers when a new peer has connected. The parameter is the name of the peer that has connected.
    LostPeer(String), // Broadcast this message to all peers when a peers has disconnected. The parameter is the name of the peer that has disconnected.
    Text,             // Standard peer text message (broadcasted message)
    PeerData, // If received by the server, a peer is asking to retrieve data about all connected peers. If received by a peer, it is the data from the server.
    PeerName(String), // The server sends this message to a client when it has first connected, giving it a random name. The name is the parameter.
    Private(String),  // A private message to the given name of a peer.
}

pub struct Client {
    addr: String,
    name: String
}

impl Client {
    pub fn new(addr: String) -> Self {
        Self { addr, name: String::new() }
    }

    pub async fn connect(&mut self) {

        let (sender, receiver) = futures::channel::mpsc::unbounded::<TungMessage>();

        let (ws_stream, _) = connect_async(format!("ws://{}/socket", &self.addr))
            .await
            .expect("Failed to connect");

        println!("WebSocket handshake has been successfully completed");

        let local_addr = ws_stream.get_ref().local_addr().unwrap().to_string();

        let (write, mut read) = ws_stream.split();

        let stdin_to_ws = receiver.map(Ok).forward(write);
        
        let mut name = String::new();

        // read first single message for name
        if let Some(msg) = read.next().await {
            let msg = msg.unwrap().to_string();
            let msg: Message = serde_json::from_str(&msg).unwrap();
            let msg_type = msg.msg_type.clone();

            match msg_type {
                MessageType::PeerName(new_name) => {
                    async_std::io::stdout()
                        .write_all(format!("\nName Assignment: {}", new_name).as_bytes())
                        .await
                        .unwrap();
                        name = new_name;
                }
                _ => {}
            }
            async_std::io::stdout().flush().await.unwrap();
        };

        task::spawn(read_stdin(sender, local_addr, name));
        
        let ws_to_stdout = async {
            while let Some(msg) = read.next().await {
                let msg = msg.unwrap().to_string();
                let msg: Message = serde_json::from_str(&msg).unwrap();
                let msg_type = msg.msg_type.clone();

                match msg_type {
                    MessageType::NewPeer(peer_n) => async_std::io::stdout()
                        .write_all(
                            format!("\n New Client: {}: {:?}", &msg.src_addr, &msg).as_bytes(),
                        )
                        .await
                        .unwrap(),
                    MessageType::LostPeer(peer_n) => async_std::io::stdout()
                        .write_all(
                            format!("\nClient disconnected: {}: {:?}", &msg.src_addr, &msg)
                                .as_bytes(),
                        )
                        .await
                        .unwrap(),
                    MessageType::Text => async_std::io::stdout()
                        .write_all(format!("\nText: {}: {:?}", &msg.src_addr, &msg).as_bytes())
                        .await
                        .unwrap(),
                    MessageType::PeerData => async_std::io::stdout()
                        .write_all(
                            format!("\nPeer data: {}: {:?}", &msg.src_addr, &msg).as_bytes(),
                        )
                        .await
                        .unwrap(),
                    MessageType::PeerName(new_name) => {

    
                        self.name = new_name.clone();
                        
                        async_std::io::stdout()
                        .write_all(format!("\nName Assignment: {}", new_name).as_bytes())
                        .await
                        .unwrap();
                    },
                    MessageType::Private(_) => async_std::io::stdout()
                        .write_all(
                            format!("\n Private message: {}: {:?}", &msg.src_addr, &msg).as_bytes(),
                        )
                        .await
                        .unwrap(),
                }

                async_std::io::stdout().flush().await.unwrap();
            }
        };

/*
        let ws_to_stdout = {
            read.for_each(|message| async {
                
            })
        };*/

        pin_mut!(stdin_to_ws, ws_to_stdout);
        future::select(stdin_to_ws, ws_to_stdout).await;
    }
}

// Our helper method which will read data from stdin and send it along the
// sender provided.
async fn read_stdin(
    sender: futures::channel::mpsc::UnboundedSender<TungMessage>,
    local_addr: String,
    peer_name: String
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

        let msg_struct = Message {
            src_addr: local_addr.to_string(),
            name: peer_name.to_string(),
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
