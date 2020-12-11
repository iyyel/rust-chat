use dotenv::dotenv;
use server::Server;
use std::env;

mod server;

fn main() {
    dotenv().ok();

    let host = env::var("HOST")
        .ok()
        .expect("Failed to parse HOST environment variable!");
    let port = env::var("PORT")
        .ok()
        .expect("Failed to parse PORT environment variable!");

    let mut server = Server::new(format!("{}:{}", host, port));
    server.run();
}
