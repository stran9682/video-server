use std::time::SystemTime;

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
use iroh_blobs::{BlobsProtocol, api::Store, store::fs::FsStore};
use iroh_docs::{api::{Doc, protocol::ShareMode}, protocol::Docs};
use iroh_gossip::Gossip;
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

    // stuff for docs
    let blobs = FsStore::load("./blobs").await?;
    let gossip = Gossip::builder().spawn(server_endpoint.clone());
    let docs = Docs::persistent("./blobs".into())
        .spawn(server_endpoint.clone(), (*blobs).clone(), gossip.clone())
        .await?;

    // Our own query stuff
    let proto = VideoUpload::new(
        &store,
        &docs
    );
    let query = Query::new(
        &store
    );
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

#[derive(Debug, Clone)]
struct VideoUpload {
    blobs: Store,
    docs: Docs
}

impl VideoUpload {
    pub fn new(blobs: &Store, docs: &Docs) -> Self {
        VideoUpload {
            docs: docs.clone(),
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

        // Create a new doc for this video
        let doc = self.docs
            .create()
            .await
            .map_err(|e| AcceptError::from_boxed(e.into()))?;

        println!("Created a doc");

        let ticket = doc
            .share(ShareMode::Write, Default::default())
            .await
            .map_err(|e| AcceptError::from_boxed(e.into()))?;

        let id_string = doc.id().to_string();

        println!("Generated a ticket: {}", ticket);

        // Copy the remote file to local
        let temp_file_name = format!("{}.mp4", connection.remote_id());
        let mut temp_file = File::create(&temp_file_name).await?;
        tokio::io::copy(&mut recv, &mut temp_file).await?;

        // Split the mp4 w. ffmpeg
        fs::create_dir(&id_string).await?;
        let args_string = format!(
            "-f segment -segment_time 1 -reset_timestamps 1 -map 0 {}/output_%d.mp4",
            id_string
        );

        let mut command = FfmpegCommand::new()
            .input(&temp_file_name)
            .args(args_string.split(' '))
            .spawn()
            .unwrap();

        command.iter().unwrap().for_each(|e| match e {
            FfmpegEvent::Log(LogLevel::Error, e) => println!("Error: {e}"),
            FfmpegEvent::Progress(p) => println!("Progress: {}", p.time),
            _ => {}
        });

        // sort the entries by time first
        let mut entries = fs::read_dir(&id_string).await?;
        let mut files = Vec::new();

        while let Some(file) = entries.next_entry().await? {
            let metadata = file.metadata().await?;
            if metadata.is_file() {
                let modified: SystemTime = metadata.modified()?;
                files.push((file.path(), modified));
            }
        }

        files.sort_by_key(|&(_, time)| time);

        send.write_all(ticket.to_string().as_bytes())
            .await
            .map_err(AcceptError::from_err)?;
        send.finish()?;

        // Add each file to iroh
        let mut file_number = 0;

        for file in files {
            let file = File::open(file.0).await?;

            let stream = ReaderStream::new(file);

            if let Err(e) = self
                .blobs
                .add_stream(stream)
                .await
                .with_named_tag(format!("{}:{}", id_string, file_number))
                .await
            {
                eprintln!("Failed to add to store: {}", e)
            }

            file_number += 1;
        }

        // return the tag and the # of clips
        println!("UUID tag: {}:{} ", id_string, file_number);

        let mut send = connection.open_uni().await?;
        // return the prefix tag to client
        send.write_all(format!("{}:{}", id_string, file_number).as_bytes()).await.unwrap(); // It's just easier using strings. Fight me.
        send.finish()?;

        connection.closed().await;

        // clean up
        fs::remove_dir_all(id_string).await?;
        fs::remove_file(&temp_file_name).await?;

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

        println!("Querying for: {}", query);

        // TODO: LOOK INTO THIS:
        // let res = self.blobs.tags().list_with_opts(ListOptions::range("h".."hello"));

        if let Some(tag) = self
            .blobs
            .tags()
            .get(&query)
            .await
            .map_err(AcceptError::from_err)?
        {
            println!("Hash associated with Tag: {}", tag.hash);

            let mut reader = self.blobs.blobs().reader(tag.hash);
            tokio::io::copy(&mut reader, &mut send).await?;
        } else {
            println!("No Hash associated with Tag: {}", query)
        }

        send.finish()?;
        connection.closed().await;

        Result::Ok(())
    }
}

