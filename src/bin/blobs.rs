use std::{str::FromStr, time::SystemTime};

use anyhow::Ok;
use ffmpeg_sidecar::{
    command::{FfmpegCommand, ffmpeg_is_installed},
    event::{FfmpegEvent, LogLevel},
};
use iroh::{
    Endpoint,
    endpoint::presets,
    protocol::{AcceptError, ProtocolHandler, Router},
};
use iroh_blobs::{
    Hash, api::{Store, blobs::AddBytesOptions}, hashseq::HashSeq, store::fs::FsStore
};
use tokio::fs::{self, File};
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;

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

    let proto = VideoUpload::new(&store);
    let query = Query::new(&store);
    proto.print().await;

    let node = Router::builder(server_endpoint)
        .accept(ALPN, proto)
        .accept("query", query)
        .spawn();

    let node_id = node.endpoint().id();
    println!("our endpoint id: {}", node_id);

    // Wait for Ctrl-C to be pressed.
    tokio::signal::ctrl_c().await?;
    node.shutdown().await?;
    Ok(())
}

#[derive(Debug, Clone)]
struct VideoUpload {
    blobs: Store,
}

impl VideoUpload {
    pub fn new(blobs: &Store) -> Self {
        VideoUpload {
            blobs: blobs.clone(),
        }
    }

    pub async fn print(&self) {
        println!("Tags:");
        let mut tags = self.blobs.tags().list().await.unwrap();
        while let Some(tag) = tags.next().await {
            let tag = tag.unwrap();
            println!("  {:?}", tag.name);
        }
        let blobs = self.blobs.list().hashes().await.unwrap();
        println!("Blobs:");
        for blob in blobs {
            println!("  {}", blob);
        }
    }
}

impl ProtocolHandler for VideoUpload {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;

        let mut temp_file = File::create("temp.mp4").await?;

        tokio::io::copy(&mut recv, &mut temp_file).await?;

        // TODO: create a separate directory for this instance.
        let args_string = "-acodec copy -f segment -segment_time 6 -vcodec copy -reset_timestamps 1 -map 0 temp/output_%d.mp4";

        let mut command = FfmpegCommand::new()
            .input("temp.mp4")
            .args(args_string.split(' '))
            .spawn()
            .unwrap();

        command.iter().unwrap().for_each(|e| match e {
            FfmpegEvent::Log(LogLevel::Error, e) => println!("Error: {e}"),
            FfmpegEvent::Progress(p) => println!("Progress: {}", p.time),
            _ => {}
        });

        // TODO: oops put this in order
        let mut entries = fs::read_dir("temp").await?;

        let mut hashes = vec![];

        let mut files = Vec::new();

        while let Some(file) = entries.next_entry().await? {
            let metadata = file.metadata().await?;
            if metadata.is_file() {
                let modified: SystemTime = metadata.modified()?;
                files.push((file.path(), modified));
            }
        }

        files.sort_by_key(|&(_, time)| time);

        for file in files {
            let file = File::open(file.0).await?;

            let stream = ReaderStream::new(file);

            let res = self.blobs.add_stream(stream).await;

            let res = res.await.unwrap();

            hashes.push(res.hash);
        }

        let hs = hashes.iter().copied().collect::<HashSeq>();
        let hash = self
            .blobs
            .add_bytes_with_opts(AddBytesOptions {
                data: hs.into(),
                format: iroh_blobs::BlobFormat::HashSeq,
            })
            .with_named_tag("temp")
            .await
            .unwrap();

        println!("Hash Sequence: {} ", hash.hash);

        send.write_all(hash.hash.as_bytes()).await.unwrap();
        send.finish().unwrap();

        connection.closed().await;

        Result::Ok(())
    }
}

#[derive(Debug, Clone)]
struct Query {
    blobs: Store,
}

impl Query {
    pub fn new(blobs: &Store) -> Self {
        Self {
            blobs: blobs.clone(),
        }
    }
}

impl ProtocolHandler for Query {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        println!("Accepted connection from {}", connection.remote_id());

        let query_bytes = recv.read_to_end(256).await.unwrap();

        let query = String::from_utf8(query_bytes).map_err(AcceptError::from_err)?;

        let hash = Hash::from_str(&query).map_err(AcceptError::from_err)?;

        println!("Querying for: {}", query);

        let mut reader = self.blobs.blobs().reader(hash);
        tokio::io::copy(&mut reader, &mut send).await.unwrap();

        // let tag = self.blobs.tags().get("temp").await.unwrap().unwrap();
        // println!("{}", tag.hash);

        send.finish()?;
        connection.closed().await;

        Result::Ok(())
    }
}