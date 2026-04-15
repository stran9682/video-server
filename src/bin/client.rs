use std::{io, str::FromStr};

use iroh::{Endpoint, EndpointId, PublicKey, endpoint::presets};
use tokio::fs::File;

const ALPN: &[u8] = b"fun";

async fn client (endpoint: EndpointId) -> anyhow::Result<()> {
    let client_endpoint = Endpoint::bind(presets::N0).await?;

    println!("Client Id is {}", client_endpoint.id());

    let conn = client_endpoint.connect(endpoint, ALPN).await?;

    let (mut send, mut _recv) = conn.open_bi().await?;

    let mut video = File::open("input.mp4").await?;

    tokio::io::copy(&mut video, &mut send).await?;

    send.finish()?;

    conn.closed().await;

    Ok(())
}

#[tokio::main]
async fn main () {

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    let node_id = PublicKey::from_str(input.trim()).unwrap();
    let node_id = EndpointId::from_bytes(&node_id).unwrap();

    if let Err(e) = client(node_id).await {
        eprintln!("Failed to send client video! {}", e)
    };
}