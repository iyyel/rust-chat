use client::Client;
use std::env;

mod client;

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
    let client = Client::new(address.to_string());

    client.connect();
}
