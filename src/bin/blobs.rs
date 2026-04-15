use anyhow::Ok;
use ffmpeg_sidecar::{command::{FfmpegCommand, ffmpeg_is_installed}, event::{FfmpegEvent, LogLevel}};
use iroh::{Endpoint, endpoint::presets, protocol::{ProtocolHandler, Router}};
use iroh_blobs::{api::Store, store::fs::FsStore};
use tokio::fs::{self, File};
use tokio_util::io::ReaderStream;

const ALPN: &[u8] = b"fun";

#[tokio::main]
async fn main () -> anyhow::Result<()> {

    ffmpeg_sidecar::download::auto_download().unwrap();

    if ffmpeg_is_installed() {
        println!("FFmpeg is already installed! 🎉");
    }

    let store = FsStore::load("./").await?;

    let server_endpoint = Endpoint::bind(presets::N0).await?;
    server_endpoint.online().await;

    let proto = VideoUpload {blobs: store.into()};

    let node = Router::builder(server_endpoint)
        .accept(ALPN, proto)
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
    blobs: Store
}

impl ProtocolHandler for VideoUpload {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        
        let (mut _send, mut recv) = connection.accept_bi().await?;
        
        let mut temp_file = File::create("temp.mp4").await?;
        
        tokio::io::copy(&mut recv, &mut temp_file).await?;

        let args_string = "-acodec copy -f segment -segment_time 2 -vcodec copy -reset_timestamps 1 -map 0 temp/output_%d.mp4";

        let mut command = FfmpegCommand::new()
            .input("temp.mp4")
            .args(args_string.split(' '))
            .spawn()
            .unwrap();

        command.iter().unwrap().for_each(|e| match e {
            FfmpegEvent::Log(LogLevel::Error, e) => println!("Error: {e}"),
            FfmpegEvent::Progress(p) => println!("Progress: {} / 00:00:15", p.time),
            _ => {}
        });

        let mut entries = fs::read_dir("/temp").await?;

        while let Some(file) = entries.next_entry().await? {
            let file = File::open(file.path()).await?;

            let stream = ReaderStream::new(file);

            let res = self.blobs.add_stream(stream).await;

            let res = res.await.unwrap();

            println!("{}", res.hash)
        }

        Result::Ok(())
    }
}