use std::net::{TcpStream};
use std::io::{Write, BufRead, BufReader};
use std::io;
use std::thread;

struct Message {
    addr: String,
    msg: String
}

pub struct Client {
    addr: String,
}

impl Client {
    pub fn new(addr: String) -> Self {
        Self {
            addr
        }
    }

    pub fn connect(&self) {
        match TcpStream::connect(&self.addr) {
            Ok(mut stream) => {
                println!("Successfully established connection to server: {}", &self.addr);
                
                let strm = stream.try_clone().unwrap();
                      // receive the echo
                      thread::spawn(move || loop {
                        let mut data = String::new();
                        let mut stream = BufReader::new(&strm);
                    
                        match stream.read_line(&mut data) {
                            Ok(_) => {
                                println!("Received: {}: {}", strm.peer_addr().unwrap(), data);
                                io::stdout().flush().unwrap();
                            }, 
                            Err(e) => {
                                println!("Error: {}", e);
                            }
                        }
                    });

                loop {
                    print!("({}) -> ", stream.peer_addr().unwrap());
                    io::stdout().flush().unwrap();
                    
                    // read input from command line
                    let msg = self.read_input();

                    // send the message
                    self.send_msg(&mut stream, msg);
                }
                
            },
            Err(e) => {
                println!("Failed to stablish connection to server: {}", e);
            }
        }
    }

    fn read_input(&self) -> String {
        let mut msg = String::new();
        loop {
            match io::stdin().read_line(&mut msg) {
                Ok(_) => {
                    return msg
                },
                Err(e) => {
                    println!("Failed reading input: {}", e);
                    continue;
                }
            }
        }
    }
    
    fn send_msg(&self, stream: &mut TcpStream, msg: String) {
        match stream.write(msg.as_bytes()) {
            Ok(_) => {
                println!("Sent: {}: {}", stream.peer_addr().unwrap(), msg);
            },
            Err(e) => {
                println!("Failed to send message: {}", e);
            }
        }
    }

 
}