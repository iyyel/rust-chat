use serde::{Deserialize, Serialize};
use std::io;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::thread;
use tungstenite::{stream::Stream, Message as TungMsg, WebSocket};
use url::Url;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    from_addr: String,
    name: String,
    msg: String,
}

pub struct Client {
    addr: String,
}

impl Client {
    pub fn new(addr: String) -> Self {
        Self { addr }
    }

    pub fn connect(&self) {
        let url = format!("ws://{}/socket", &self.addr);
        match tungstenite::connect(Url::parse(&url).unwrap()) {
            Ok((mut websocket, response)) => {
                println!(
                    "Successfully established connection to server: {}",
                    &self.addr
                );
                
                // receive server messages
                thread::spawn(move || loop {
                    loop {
                        let msg = &websocket.read_message().expect("Error reading message");
                        println!("Received: {}", msg);
                    }
                });

                loop {
                    print!("\n{}: ", "local address");
                    io::stdout().flush().unwrap();

                    // read input from command line
                    let msg = self.read_input();

                    let msg = Message {
                        from_addr: "local address".to_string(),
                        name: "local address".to_string(),
                        msg,
                    };

                    // send the message
                    let msg_str = serde_json::to_string(&msg).unwrap();
                    websocket.write_message(TungMsg::Text(msg_str.into()));
                }
            }
            Err(e) => {
                println!("Failed to stablish connection to server: {}", e);
            }
        }

        /*
        match TcpStream::connect(&self.addr) {
            Ok(mut stream) => {
                println!(
                    "Successfully established connection to server: {}",
                    &self.addr
                );

                let strm = stream.try_clone().unwrap();

                // receive server messages
                thread::spawn(move || loop {
                    let mut buf = String::new();
                    let mut reader = BufReader::new(&strm);

                    match reader.read_line(&mut buf) {
                        Ok(_) => {
                            let msg: Message = serde_json::from_str(&buf).unwrap();
                            println!("\n{}: {:?}", msg.from_addr, msg);
                            io::stdout().flush().unwrap();
                        }
                        Err(e) => {
                            println!("Error: {}", e);
                        }
                    }
                });

                loop {
                    print!("\n{}: ", stream.local_addr().unwrap());
                    io::stdout().flush().unwrap();

                    // read input from command line
                    let msg = self.read_input();

                    let msg = Message {
                        from_addr: stream.local_addr().unwrap().to_string(),
                        name: stream.local_addr().unwrap().to_string(),
                        msg,
                    };

                    // send the message
                    self.send_msg(&mut stream, msg);
                }
            }
            Err(e) => {
                println!("Failed to stablish connection to server: {}", e);
            }
        }
        */
    }

    fn read_input(&self) -> String {
        let mut msg = String::new();
        loop {
            match io::stdin().read_line(&mut msg) {
                Ok(_) => return msg,
                Err(e) => {
                    println!("Failed reading input: {}", e);
                    continue;
                }
            }
        }
    }
}
