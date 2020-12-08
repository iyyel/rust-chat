use std::env;

use server::Server;

mod server;

fn check_args(args: &Vec<String>) {
    if args.len() != 2 {
        println!("Please provide server address!");
        std::process::exit(1);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    check_args(&args);

    let address = &args[1];
    let mut server = Server::new(address.to_string());

    server.run();
}
