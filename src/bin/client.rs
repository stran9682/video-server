use std::{io, str::FromStr};

use iroh::{Endpoint, EndpointId, PublicKey, endpoint::presets, protocol::Router};
use iroh_blobs::{BlobsProtocol, store::mem::MemStore};
use iroh_docs::{DocTicket, protocol::Docs};
use iroh_gossip::Gossip;
use tokio::fs::File;

const ALPN: &[u8] = b"fun";

async fn upload(
    client_endpoint: &Endpoint,
    server_endpoint: &EndpointId,
    docs: Docs,
) -> anyhow::Result<()> {
    println!("Client Id is {}", client_endpoint.id());

    let conn = client_endpoint.connect(*server_endpoint, ALPN).await?;

    let (mut send, mut recv) = conn.open_bi().await?;

    let mut video = File::open("input.mp4").await?;

    tokio::io::copy(&mut video, &mut send).await?;

    send.finish()?;

    let bytes = recv.read_to_end(256).await?;
    let ticket_str = str::from_utf8(&bytes)?;
    println!("Received a ticket: {}", ticket_str);
    let ticket = DocTicket::from_str(ticket_str.trim())?;

    let mut recv = conn.accept_uni().await?;
    let bytes = recv.read_to_end(256).await?;
    let tag = String::from_utf8(bytes)?;
    println!("Doc ID : # Clips: {}", tag);

    let doc = docs.import(ticket).await?;
    let author = docs.author_create().await?; // TODO adjust this
    doc.set_bytes(author, tag, client_endpoint.id().to_string())
        .await?;

    conn.close(0u32.into(), b"all done!");

    Ok(())
}

async fn query(
    client_endpoint: &Endpoint,
    server_endpoint: &EndpointId,
    tag: &str,
) -> anyhow::Result<()> {
    let conn = client_endpoint.connect(*server_endpoint, b"query").await?;

    let (mut send, mut recv) = conn.open_bi().await?;

    send.write_all(tag.as_bytes()).await?;
    send.finish()?;

    let mut hash_bytes = [0u8; 32];

    println!("Connection opened");

    loop {
        match recv.read_exact(&mut hash_bytes).await {
            Err(iroh::endpoint::ReadExactError::FinishedEarly(_)) => break,
            Err(err) => return Err(err.into()),
            Ok(_) => {}
        };
        println!("{:?}", hash_bytes)
    }
    conn.close(0u32.into(), b"done");

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Enter endpoint: ");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    let server_id = PublicKey::from_str(input.trim())?;

    // stuff for docs
    let client_endpoint = Endpoint::bind(presets::N0).await?;
    let blobs = MemStore::default();
    let gossip = Gossip::builder().spawn(client_endpoint.clone());
    let docs = Docs::memory()
        .spawn(client_endpoint.clone(), (*blobs).clone(), gossip.clone())
        .await?;

    // register all three protocols on the router
    let router = Router::builder(client_endpoint.clone())
        .accept(iroh_blobs::ALPN, BlobsProtocol::new(&blobs, None))
        .accept(iroh_gossip::ALPN, gossip)
        .accept(iroh_docs::ALPN, docs.clone())
        .spawn();

    if let Err(e) = upload(&client_endpoint, &server_id, docs).await {
        eprintln!("Failed to send client video! {}", e)
    };

    input.clear();
    println!("Query for tag?");
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    if let Err(e) = query(&client_endpoint, &server_id, input.trim()).await {
        eprintln!("Failed to query server! {}", e)
    };

    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    Ok(())
}
