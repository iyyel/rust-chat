use async_std::task;
use dotenv::dotenv;
use server::Server;
use std::{env, io::Error as IoError};

mod server;

fn main() -> Result<(), IoError> {
    dotenv().ok();

    let host = env::var("HOST")
        .ok()
        .expect("Failed to parse HOST environment variable!");
    let port = env::var("PORT")
        .ok()
        .expect("Failed to parse PORT environment variable!");

    let server = Server::new(format!("{}:{}", host, port));
    task::block_on(server.run())
}
