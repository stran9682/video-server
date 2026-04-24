use std::{io, str::FromStr};

use anyhow::Ok;
use iroh::{Endpoint, endpoint::presets, protocol::{AcceptError, ProtocolHandler, Router}};
use iroh_blobs::{BlobsProtocol, store::mem::MemStore};
use iroh_docs::{NamespaceId, api::protocol::ShareMode, engine::LiveEvent, protocol::Docs, store::Query};
use iroh_gossip::Gossip;
use tokio_stream::StreamExt;

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

    // create a document
    let doc = docs.create().await?;

    let ticket = doc.share(ShareMode::Write, Default::default()).await?;

    println!("share this ticket with a peer: {}", ticket);

    println!("Enter to continue: ");
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read line");

    if let Some(entry) = doc.get_one(Query::single_latest_per_key().key_exact("greeting")).await? {
        // the entry holds the hash; fetch the actual bytes from the blobs store
        let bytes = blobs.blobs().get_bytes(entry.content_hash()).await?;
        println!("{}", std::str::from_utf8(&bytes)?);
    }

    

    // Wait for Ctrl-C to be pressed.
    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    Ok(())
}

#[derive(Debug, Clone)]
struct AccessControl {
    docs: Docs
}

impl ProtocolHandler for AccessControl {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {

        let (mut send, mut recv) = connection.accept_bi().await?;

        let res = send.write_all(b"yo").await;

        Result::Ok(())
    }   
}

