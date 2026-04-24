use std::{io, str::FromStr};
use std::result::Result::Ok;

use iroh::{Endpoint, endpoint::presets, protocol::Router};
use iroh_blobs::{BlobsProtocol, store::mem::MemStore};
use iroh_docs::{DocTicket, protocol::Docs};
use iroh_gossip::Gossip;

#[tokio::main]
async fn main() -> anyhow::Result<()> { 

    let endpoint = Endpoint::bind(presets::N0).await?;

    let blobs = MemStore::default();

    let gossip = Gossip::builder().spawn(endpoint.clone());
    
    let docs = Docs::memory()
        .spawn(endpoint.clone(), (*blobs).clone(), gossip.clone())
        .await?;

    // register all three protocols on the router
    let router = Router::builder(endpoint.clone())
        .accept(iroh_blobs::ALPN, BlobsProtocol::new(&blobs, None))
        .accept(iroh_gossip::ALPN, gossip)
        .accept(iroh_docs::ALPN, docs.clone())
        .spawn();

    println!("Enter Ticket: ");

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    let ticket = DocTicket::from_str(&input.trim())?;
    let Ok(doc) = docs.import(ticket).await else {
        println!("failed");
        return anyhow::Ok(())
    };
    
    let author = docs.author_create().await?;
    doc.set_bytes(author, b"greeting".to_vec(), "hello, world").await?;

    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    anyhow::Ok(())
}