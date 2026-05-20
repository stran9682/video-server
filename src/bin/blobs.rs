use std::{
    io::{self, Error},
    str::FromStr,
};

use anyhow::Ok;
use ffmpeg_sidecar::command::ffmpeg_is_installed;
use iroh::{Endpoint, endpoint::presets, protocol::Router};
use iroh_blobs::{BlobsProtocol, store::fs::FsStore};
use iroh_docs::{DocTicket, api::protocol::ShareMode, protocol::Docs};
use iroh_gossip::Gossip;
use n0_future::StreamExt;
use server::{
    protocols::{QueryProtocol, VideoUpload},
    util::get_key,
};

const ALPN: &[u8] = b"fun";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ffmpeg_sidecar::download::auto_download().unwrap();

    if ffmpeg_is_installed() {
        println!("FFmpeg is already installed! 🎉");
    }

    let store = FsStore::load("./").await?;

    let secret_key = get_key().await?;

    let server_endpoint = iroh::Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .bind()
        .await?;

    server_endpoint.online().await;

    // stuff for docs
    let blobs = FsStore::load("./blobs").await?;
    let gossip = Gossip::builder().spawn(server_endpoint.clone());
    let docs = Docs::persistent("./blobs".into())
        .spawn(server_endpoint.clone(), (*blobs).clone(), gossip.clone())
        .await?;

    let mut res = docs.list().await?;
    while let Some(entry) = res.next().await {
        let (namespace_id, _) = entry?;

        let doc = docs.open(namespace_id).await?.ok_or(Error::new(
            io::ErrorKind::InvalidFilename,
            "Couldn't open document",
        ))?;

        let ticket = doc.share(ShareMode::Write, Default::default()).await?;
        let ticket_str = ticket.to_string();

        // no way you can do this.
        // bahah using your document to rejoin
        println!("Doc ticket on startup: {}", ticket.to_string());
        docs.import(DocTicket::from_str(&ticket_str)?).await?;
    }

    // Our own query stuff
    let proto = VideoUpload::new(&store, &docs);
    let query = QueryProtocol::new(&store, &docs, &blobs);
    proto.print().await;

    // oh dear
    let node = Router::builder(server_endpoint)
        .accept(ALPN, proto)
        .accept("query", query)
        .accept(iroh_docs::ALPN, docs.clone())
        .accept(iroh_blobs::ALPN, BlobsProtocol::new(&blobs, None))
        .accept(iroh_gossip::ALPN, gossip)
        .spawn();

    let node_id = node.endpoint().id();
    println!("our endpoint id: {}", node_id);

    // Wait for Ctrl-C to be pressed.
    tokio::signal::ctrl_c().await?;
    node.shutdown().await?;
    Ok(())
}
