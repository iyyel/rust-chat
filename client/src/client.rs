use futures::{future, pin_mut, StreamExt};

use serde::{Deserialize, Serialize};
use async_std::io;

use async_std::prelude::*;
use async_std::task;
use async_tungstenite::async_std::connect_async;
use async_tungstenite::tungstenite::protocol::Message as TungMessage;

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

pub struct Client {
    addr: String,
}

impl Client {
    pub fn new(addr: String) -> Self {
        Self { addr }
    }

    pub async fn connect(&self) {
        let (sender, receiver) = futures::channel::mpsc::unbounded::<TungMessage>();

        let (ws_stream, _) = connect_async(format!("ws://{}/socket", &self.addr))
            .await
            .expect("Failed to connect");

        println!("WebSocket handshake has been successfully completed");

        let local_addr = ws_stream.get_ref().local_addr().unwrap().to_string();
        task::spawn(read_stdin(sender, local_addr));

        let (write, read) = ws_stream.split();

        let stdin_to_ws = receiver.map(Ok).forward(write);
        let ws_to_stdout = {
            read.for_each(|message| async {
                let data = message.unwrap().to_string();

                // unneccessary conversation.. :)
                let msg: Message = serde_json::from_str(&data).unwrap();
            
                async_std::io::stdout().write_all(format!("\n{}: {:?}", &msg.src_addr, &msg).as_bytes()).await.unwrap();
                async_std::io::stdout().flush().await.unwrap();
            })
        };

        pin_mut!(stdin_to_ws, ws_to_stdout);
        future::select(stdin_to_ws, ws_to_stdout).await;
    }
}

// Our helper method which will read data from stdin and send it along the
// sender provided.
async fn read_stdin(sender: futures::channel::mpsc::UnboundedSender<TungMessage>, local_addr: String) {
    let mut stdin = io::stdin();
    loop {
        async_std::io::stdout().write_all(format!("\n{}: ", local_addr).as_bytes()).await.unwrap();
        async_std::io::stdout().flush().await.unwrap();
       
        let mut buf = vec![0; 1024];
        let n = match stdin.read(&mut buf).await {
            Err(_) | Ok(0) => break,
            Ok(n) => n,
        };
        buf.truncate(n);

        let msg = String::from_utf8(buf).unwrap();

        if msg.starts_with(":n ") {
            let name = msg[3..].to_string();
            let msg_struct = Message {
                src_addr: local_addr.clone(),
                name: local_addr.clone(),
                msg_type: MessageType::ChangeName(name),
                msg: "Namechange!".to_string()
            };
            sender.unbounded_send(TungMessage::Text(serde_json::to_string(&msg_struct).unwrap())).unwrap();
        } else {
            let msg_struct = Message {
                src_addr: local_addr.clone(),
                name: local_addr.clone(),
                msg_type: MessageType::Broadcast,
                msg: msg,
            };
            sender.unbounded_send(TungMessage::Text(serde_json::to_string(&msg_struct).unwrap())).unwrap();
        }

    }
}