use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh_blobs::api::Store;
use iroh_docs::protocol::Docs;
use tokio::fs::{self, File};
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;

use crate::util::{check_permissions, create_doc, split_video_file};

#[derive(Debug, Clone)]
pub struct VideoUpload {
    blobs: Store,
    docs: Docs,
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

        // Create a iroh-doc.
        // This will store who's allowed to view this video.
        // Everything will be indentified using the doc id
        let (doc, ticket) = create_doc(&self.docs)
            .await
            .map_err(|e| AcceptError::from_boxed(e.into()))?;
        let doc_id_string = doc.id().to_string();

        // Copy the sender's file to local,
        // then split it using ffmpeg
        let temp_file_name = format!("{}.mp4", doc_id_string);
        let mut temp_file = File::create(&temp_file_name).await?;
        tokio::io::copy(&mut recv, &mut temp_file).await?;
        let files = split_video_file(&doc_id_string)
            .await
            .map_err(|e| AcceptError::from_boxed(e.into()))?;

        // Send the ticket so the client can begin syncing
        send.write_all(ticket.to_string().as_bytes())
            .await
            .map_err(AcceptError::from_err)?;
        send.finish()?;

        // Add each file to iroh-blobs
        let mut file_number = 0;
        for file in files {
            let file = File::open(file).await?;

            let stream = ReaderStream::new(file);

            if let Err(e) = self
                .blobs
                .add_stream(stream)
                .await
                .with_named_tag(format!("{}:{}", doc_id_string, file_number))
                .await
            {
                eprintln!("Failed to add to store: {}", e)
            }

            file_number += 1;
        }

        // return the tag and the # of clips
        println!("video tag: {}:{} ", doc_id_string, file_number);
        let mut send = connection.open_uni().await?;
        send.write_all(format!("{}:{}", doc_id_string, file_number).as_bytes())
            .await
            .unwrap(); // It's just easier using strings. Fight me.
        send.finish()?;

        connection.closed().await;

        // clean up
        fs::remove_dir_all(doc_id_string).await?;
        fs::remove_file(&temp_file_name).await?;

        Result::Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct QueryProtocol {
    blobs: Store,
    docs: Docs,
    docs_store: Store,
}

impl QueryProtocol {
    pub fn new(blobs: &Store, docs: &Docs, docs_store: &Store) -> Self {
        Self {
            blobs: blobs.clone(),
            docs: docs.clone(),
            docs_store: docs_store.clone(),
        }
    }
}

impl ProtocolHandler for QueryProtocol {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await?;
        println!("Accepted connection from {}", connection.remote_id());

        let query_bytes = recv.read_to_end(256).await.unwrap();

        let query = String::from_utf8(query_bytes).map_err(AcceptError::from_err)?;

        println!("Querying for: {}", query);
        let namespace_str = query.split(":").collect::<Vec<&str>>()[0];
        let authorized = check_permissions(
            &self.docs,
            &self.docs_store,
            namespace_str,
            &connection.remote_id().to_string(),
        )
        .await
        .map_err(|e| AcceptError::from_boxed(e.into()))?;

        println!("Authorized? : {} ", authorized);

        if let Some(tag) = self
            .blobs
            .tags()
            .get(query.trim())
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
