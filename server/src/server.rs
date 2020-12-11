use serde::{Deserialize, Serialize};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tungstenite::{server, Message as TungMsg};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    from_addr: String,
    name: String,
    msg: String,
}

pub struct Server {
    addr: String,
    clients: Vec<TcpStream>,
}

impl Server {
    pub fn new(addr: String) -> Self {
        Self {
            addr,
            clients: Vec::new(),
        }
    }

    pub fn run(&mut self) {
        let listener = TcpListener::bind(&self.addr).unwrap();

        println!("Listening on address: {}", self.addr);

        listener
            .set_nonblocking(true)
            .expect("Failed to set non-blocking state.");

        let (sender, reciever) = mpsc::channel::<Message>();

        loop {
            // accept incoming tcp connections
            if let Ok((stream, _)) = listener.accept() {
                &self.on_client_connect(stream, &sender);
            }

            // broadcast message when one is received from any sender
            if let Ok(msg) = reciever.try_recv() {
                println!("omfg");
                &self.broadcast_msg(msg);
            }

            thread::sleep(Duration::from_millis(200));
        }
    }

    fn on_client_connect(&mut self, mut stream: TcpStream, sender: &mpsc::Sender<Message>) {
        let peer_addr = stream.peer_addr().unwrap().to_string();
        let local_addr = stream.local_addr().unwrap().to_string();

        let sender = sender.clone();

        let new_client_msg = Message {
            from_addr: local_addr.clone(),
            name: "Server".to_string(),
            msg: format!("Client {:?} has connected.", peer_addr),
        };

        &self.broadcast_msg(new_client_msg);
        println!("\nClient {:?} has connected.", peer_addr);

        &self
            .clients
            .push(stream.try_clone().expect("Failed to clone client stream."));

        thread::spawn(move || loop {
            handle_msg(&mut stream, &sender)
        });
    }

    fn broadcast_msg(&self, msg: Message) {
        let mut msg_str = serde_json::to_string(&msg).unwrap();
        // This line of code is required, as the current Rust client
        // reads a whole line, so we need to make sure that a newline character is
        // at the end of the message. This can possibly be done better?
        msg_str.extend("\n".chars());

        &self
            .clients
            .iter()
            .filter_map(|mut stream| {
                match stream.peer_addr() {
                    Ok(addr) => {
                        if addr.to_string() != msg.from_addr {
                            println!("\nBroadcast: {:?}", msg);
                            let buf = msg_str.clone().into_bytes();
                            buf.clone().resize(buf.len(), 0);
                            return stream.write_all(&buf).map(|_| stream).ok();
                        }
                        /*
                        if addr.to_string() != msg.from_addr {
                            let mut websocket = server::accept(stream).unwrap();
                            println!("\nBroadcast: {:?}", msg);
                            let buf = msg_str.clone().into_bytes();
                            buf.clone().resize(buf.len(), 0);

                            websocket.write_message(TungMsg::Text(msg_str.into())).unwrap();

                            return stream.write_all(&buf).map(|_| stream).ok();
                        }
                        */
                    }
                    Err(e) => println!("Failed to broadcast: {}", e),
                }
                None
            })
            .collect::<Vec<_>>();
    }
}

fn send_disconnect_msg(local_addr: &String, peer_addr: &String, sender: &mpsc::Sender<Message>) {
    let msg = Message {
        from_addr: local_addr.clone(),
        name: "Server".to_string(),
        msg: format!("Client {} has disconnected.", peer_addr),
    };

    println!("\nClient: {} has disconnected.", peer_addr);
    sender.send(msg).expect("failed to send msg to reciever");
}

fn handle_msg(stream: &mut TcpStream, sender: &mpsc::Sender<Message>) {
    let peer_addr = stream.peer_addr().unwrap().to_string();
    let local_addr = stream.local_addr().unwrap().to_string();
    println!("hi!sdfsfdsf");
    let mut websocket = server::accept(stream.try_clone().unwrap()).unwrap();

    match websocket.read_message() {
        Ok(msg) => match msg {
            TungMsg::Text(text) => {
                let msg: Message = serde_json::from_str(&text).unwrap();
                println!("\n{}: {:?}", &peer_addr, msg);
                sender.send(msg).expect("failed to send msg to reciever");
            }
            TungMsg::Binary(bin) => println!("\nReceived binary: {}: {:?}", &peer_addr, bin),
            TungMsg::Close(_) => send_disconnect_msg(&local_addr, &peer_addr, &sender),
            TungMsg::Pong(payload) => println!("\nReceived pong: {}: {:?}", &peer_addr, payload),
            TungMsg::Ping(payload) => println!("\nReceived ping: {}: {:?}", &peer_addr, payload),
        },
        Err(e) => println!("\nError while handling msg: {}: {:?}", &peer_addr, e),
    }

    thread::sleep(Duration::from_millis(200));
}