use anyhow::Ok;
use ffmpeg_sidecar::command::ffmpeg_is_installed;
use iroh::{Endpoint, endpoint::presets, protocol::Router};
use iroh_blobs::{BlobsProtocol, store::fs::FsStore};
use iroh_docs::protocol::Docs;
use iroh_gossip::Gossip;
use server::protocols::{QueryProtocol, VideoUpload};

const ALPN: &[u8] = b"fun";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ffmpeg_sidecar::download::auto_download().unwrap();

    if ffmpeg_is_installed() {
        println!("FFmpeg is already installed! 🎉");
    }

    let store = FsStore::load("./").await?;

    let server_endpoint = Endpoint::bind(presets::N0).await?;
    server_endpoint.online().await;

    // stuff for docs
    let blobs = FsStore::load("./blobs").await?;
    let gossip = Gossip::builder().spawn(server_endpoint.clone());
    let docs = Docs::persistent("./blobs".into())
        .spawn(server_endpoint.clone(), (*blobs).clone(), gossip.clone())
        .await?;

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
