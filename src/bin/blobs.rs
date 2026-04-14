use anyhow::Ok;
use ffmpeg_sidecar::{command::{FfmpegCommand, ffmpeg_is_installed}, event::{FfmpegEvent, LogLevel}};
use iroh::{Endpoint, EndpointId, endpoint::presets, protocol::{ProtocolHandler, Router}};
use tokio::{fs::File};

const ALPN: &[u8] = b"fun";

#[tokio::main]
async fn main () -> anyhow::Result<()> {
    let server_endpoint = Endpoint::bind(presets::N0).await?;

    let proto = VideoUpload::new();

    let node = Router::builder(server_endpoint)
        .accept(ALPN, proto)
        .spawn();

    let node_id = node.endpoint().id();
    println!("our endpoint id: {node_id}");

    tokio::spawn(async move {
        if let Err(e) = client(node_id).await {
            eprintln!("Failed to send client video! {}", e)
        };
    });

    // Wait for Ctrl-C to be pressed.
    tokio::signal::ctrl_c().await?;
    node.shutdown().await?;
    Ok(())
}

async fn client (endpoint: EndpointId) -> anyhow::Result<()> {
    let client_endpoint = Endpoint::bind(presets::N0).await?;

    let conn = client_endpoint.connect(endpoint, ALPN).await?;

    let (mut send, mut _recv) = conn.open_bi().await?;

    let mut video = File::open("input.mp4").await?;

    tokio::io::copy(&mut video, &mut send).await?;

    Ok(())
}


#[derive(Debug, Clone)]
struct VideoUpload { }

impl ProtocolHandler for VideoUpload {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        
        let (mut _send, mut recv) = connection.accept_bi().await?;
        
        let mut temp_file = File::create("temp.mp4").await?;
        
        tokio::io::copy(&mut recv, &mut temp_file).await?;

        let args_string = "-acodec copy -f segment -segment_time 30 -vcodec copy -reset_timestamps 1 -map 0 temp/output_%d.mp4";

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

        Result::Ok(())
    }
}

impl VideoUpload {
    fn new () -> Self {
        ffmpeg_sidecar::download::auto_download().unwrap();

        if ffmpeg_is_installed() {
            println!("FFmpeg is already installed! 🎉");
        }

        VideoUpload {  }
    }
}