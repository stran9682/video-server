use iroh::{Endpoint, EndpointId, endpoint::presets};
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

    Ok(())
}

#[tokio::main]
async fn main () {

    let node_id: [u8; 32] = [75, 62, 157, 243, 135, 63, 219, 62, 45, 97, 226, 111, 9, 219, 91, 138, 74, 183, 54, 49, 24, 103, 205, 196, 79, 201, 239, 157, 56, 32, 135, 196];
    let node_id = EndpointId::from_bytes(&node_id).unwrap();


    if let Err(e) = client(node_id).await {
        eprintln!("Failed to send client video! {}", e)
    };
}