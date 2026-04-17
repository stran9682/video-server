use ffmpeg_sidecar::{
    command::{FfmpegCommand, ffmpeg_is_installed},
    event::{FfmpegEvent, LogLevel},
};

fn main() {
    ffmpeg_sidecar::download::auto_download().unwrap();

    if ffmpeg_is_installed() {
        println!("FFmpeg is already installed! 🎉");
    }

    let args_string = "-acodec copy -f segment -segment_time 2 -vcodec copy -reset_timestamps 1 -map 0 temp/output_time_%d.mp4";

    let mut command = FfmpegCommand::new()
        .input("input.mp4")
        .args(args_string.split(' '))
        .spawn()
        .unwrap();

    command.iter().unwrap().for_each(|e| match e {
        FfmpegEvent::Log(LogLevel::Error, e) => println!("Error: {e}"),
        FfmpegEvent::Progress(p) => println!("Progress: {} / 00:00:15", p.time),
        _ => {}
    });
}
