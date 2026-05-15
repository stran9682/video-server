use std::{path::PathBuf, str::FromStr, time::SystemTime};

use anyhow::Ok;
use ffmpeg_sidecar::{
    command::FfmpegCommand,
    event::{FfmpegEvent, LogLevel},
};
use iroh_blobs::api::Store;
use iroh_docs::{
    DocTicket, NamespaceId,
    api::{Doc, protocol::ShareMode},
    protocol::Docs,
    store::Query,
};
use serde::{Deserialize, Serialize};
use tokio::fs::{self};

pub async fn split_video_file(doc_id_string: &str) -> anyhow::Result<Vec<PathBuf>> {
    // Split the mp4 w. ffmpeg
    fs::create_dir(&doc_id_string).await?;
    let args_string = format!(
        "-f segment -segment_time 1 -reset_timestamps 1 -map 0 {}/output_%d.mp4",
        doc_id_string
    );

    let temp_file_name = format!("{}.mp4", doc_id_string);
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
    let mut entries = fs::read_dir(&doc_id_string).await?;
    let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();

    while let Some(file) = entries.next_entry().await? {
        let metadata = file.metadata().await?;
        if metadata.is_file() {
            let modified: SystemTime = metadata.modified()?;
            files.push((file.path(), modified));
        }
    }

    files.sort_by_key(|&(_, time)| time);
    let paths: Vec<PathBuf> = files.into_iter().map(|entry| entry.0).collect();

    Ok(paths)
}

pub async fn create_doc(docs: &Docs) -> anyhow::Result<(Doc, DocTicket)> {
    // Create a new doc for this video
    let doc = docs.create().await?;

    println!("Created a doc");

    let ticket = doc.share(ShareMode::Write, Default::default()).await?;

    println!("Generated a ticket: {}", ticket);

    Ok((doc, ticket))
}

pub async fn check_permissions(
    docs: &Docs,
    blobs: &Store,
    namespace_id: &str,
    endpoint_id: &str,
) -> anyhow::Result<bool> {
    let namespace = NamespaceId::from_str(&namespace_id)?;
    let doc: Doc = docs.open(namespace).await?.unwrap();

    println!("Document does exist");

    // we only have one entry per document, so this should be fine.
    if let Some(entry) = doc
        .get_one(Query::single_latest_per_key().key_exact("accesslist"))
        .await?
    {
        println!("Found an entry inside document");

        let bytes = blobs.get_bytes(entry.content_hash()).await.inspect_err(|e| eprintln!("{e}"))?;

        println!("Bytes for entry does exist");

        // TODO: deserialize the JSON
        let authorized_users: AuthorizedUsers = serde_json::from_slice(&bytes)?;

        println!("Was in JSON format");

        println!("authorized users: {:?}", authorized_users.authorized_users);

        return Ok(authorized_users.authorized_users.contains(&endpoint_id.to_owned()));
    }

    Ok(false)
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct AuthorizedUsers {
    pub authorized_users: Vec<String> 
}
