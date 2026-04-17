use std::{io, str::FromStr};

use iroh::{Endpoint, EndpointId, PublicKey, endpoint::presets};
use iroh_blobs::{api::Store, Hash};
use tokio::{fs::File, io::AsyncWriteExt};

const ALPN: &[u8] = b"fun";

async fn upload(client_endpoint: &Endpoint, server_endpoint: &EndpointId) -> anyhow::Result<()> {

    println!("Client Id is {}", client_endpoint.id());

    let conn = client_endpoint.connect(*server_endpoint, ALPN).await?;

    let (mut send, mut _recv) = conn.open_bi().await?;

    let mut video = File::open("input.mp4").await?;

    tokio::io::copy(&mut video, &mut send).await?;

    send.finish()?;

    conn.closed().await;

    Ok(())
}

async fn query(client_endpoint: &Endpoint, server_endpoint: &EndpointId, hash: &str) -> anyhow::Result<()> {
    let conn = client_endpoint.connect(*server_endpoint, b"query").await?;

    let (mut send, mut recv) = conn.open_bi().await?;

    send.write_all(hash.as_bytes()).await?;
    send.finish()?;

    let mut hash_bytes = [0u8; 32];

    println!("Connection opened");
    
    loop {
        match recv.read_exact(&mut hash_bytes).await {
            Err(iroh::endpoint::ReadExactError::FinishedEarly(_)) => break,
            Err(err) => return Err(err.into()),
            Ok(_) => {}
        };
        let hash = Hash::from_bytes(hash_bytes);
        println!("{}", hash)
    }
    conn.close(0u32.into(), b"done");

    Ok(())
}

#[tokio::main]
async fn main() {

    println!("Enter endpoint: ");

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    let server_id = PublicKey::from_str(input.trim()).unwrap();
    let server_id = EndpointId::from_bytes(&server_id).unwrap();

    let client_endpoint = Endpoint::bind(presets::N0).await.unwrap();

    // if let Err(e) = upload(&client_endpoint, &server_id).await {
    //     eprintln!("Failed to send client video! {}", e)
    // };
    
    input.clear();
    println!("Query for hash?");
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    if let Err(e) = query(&client_endpoint, &server_id, input.trim()).await {
        eprintln!("Failed to query server! {}", e)
    };
}

async fn read_to_file(store: &Store, hash: Hash) -> anyhow::Result<()> {
    let mut content = store.get_bytes(hash).await?;

    let mut clip = File::create("clip.mp4").await?;
    clip.write_all_buf(&mut content).await?;

    Ok(())
}