use async_std::task;
use client::Client;
use dotenv::dotenv;
use std::env;

mod client;

fn main() {
    dotenv().ok();

    let host = env::var("HOST")
        .ok()
        .expect("Failed to parse HOST environment variable!");
    let port = env::var("PORT")
        .ok()
        .expect("Failed to parse PORT environment variable!");

    let mut client = Client::new(format!("{}:{}", host, port));

    task::block_on(client.connect());
}
