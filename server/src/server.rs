use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    from_addr: SocketAddr,
    name: String,
    msg: String
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
            .expect("Failed to set Non-Blocking state.");

        let (sender, reciever) = mpsc::channel::<String>();

        loop {
            // accept incoming connections
            if let Ok((stream, _)) = listener.accept() {
                &self.on_client_connect(stream, &sender);
            }

            // broadcast message
            if let Ok(msg) = reciever.try_recv() {
                let msg: Message = serde_json::from_str(&msg).unwrap();
                &self.broadcast_msg(msg);
            }

            thread::sleep(Duration::from_millis(200));
        }
    }

    fn on_client_connect(&mut self, mut stream: TcpStream, sender: &mpsc::Sender<String>) {
        let addr = stream.peer_addr().unwrap();
        println!("Client {:?} has connected.", addr);

        let sender = sender.clone();
        //Clone the socket to push it into a thread.
        &self
            .clients
            .push(stream.try_clone().expect("Failed to clone the client."));

        thread::spawn(move || loop {
            //Create a buffer to store the msges.
            let mut buf = vec![0 as u8; 1024];

            //Hear socket entries from sender an match it with a Result.
            match stream.read(&mut buf) {
                //a read() syscall on a socket that has been closed on the other end will return 0 bytes read,
                //but no error, which should translate to Ok(0) in Rust.
                //But this may only apply when the other end closed the connection cleanly.
                Ok(0) => {
                    println!("\nClient: {} has disconnected.", addr);
                    break;
                }
                //Handle when we do not read an empty socket
                Ok(_) => {
                    //Set the buffer as an Iretartor and take it's elements while the condition retunrs true. Finally returns a Vec of type T
                    let msg = buf
                        .clone()
                        .into_iter()
                        .take_while(|&x| x != 0)
                        .collect::<Vec<_>>();

                    let msg = String::from_utf8(msg).expect("Invalid utf8 message");

                    let msg_struct: Message = serde_json::from_str(&msg).unwrap();

                    println!("\n{}: {:?}", addr, msg_struct);

                    sender.send(msg).expect("failed to send msg to reciever");
                }
                //Handle reading errors!
                Err(ref err) if err.kind() == ErrorKind::WouldBlock => (),
                Err(_) => {
                    println!("\nClient: {} has disconnected.", addr);
                    break;
                }
            }

            thread::sleep(Duration::from_millis(200));
        });
    }

    fn broadcast_msg(&self, msg: Message) {
        &self
            .clients
            .iter()
            .filter_map(|mut client| {
                let buf = serde_json::to_string(&msg).unwrap().into_bytes();
                buf.clone().resize(buf.len(), 0);
                client.write_all(&buf).map(|_| client).ok()
            })
            .collect::<Vec<_>>();
    }
}