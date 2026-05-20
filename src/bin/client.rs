use std::{io, str::FromStr};

use iroh::{Endpoint, EndpointId, PublicKey, endpoint::presets, protocol::Router};
use iroh_blobs::{
    BlobsProtocol,
    store::{fs::FsStore, mem::MemStore},
};
use iroh_docs::{DocTicket, engine::LiveEvent, protocol::Docs};
use iroh_gossip::Gossip;
use n0_future::StreamExt;
use server::util::AuthorizedUsers;
use tokio::fs::File;

const ALPN: &[u8] = b"fun";

async fn upload(
    client_endpoint: &Endpoint,
    server_endpoint: &EndpointId,
    docs: Docs,
) -> anyhow::Result<()> {
    println!("Client Id is {}", client_endpoint.id());

    let conn = client_endpoint.connect(*server_endpoint, ALPN).await?;

    let (mut send, mut recv) = conn.open_bi().await?;

    let mut video = File::open("input.mp4").await?;

    tokio::io::copy(&mut video, &mut send).await?;

    send.finish()?;

    let bytes = recv.read_to_end(256).await?;
    let ticket_str = str::from_utf8(&bytes)?;
    println!("Received a ticket: {}", ticket_str);
    let ticket = DocTicket::from_str(ticket_str.trim())?;

    let mut recv = conn.accept_uni().await?;
    let bytes = recv.read_to_end(256).await?;
    let tag = String::from_utf8(bytes)?;
    println!("Doc ID : # Clips: {}", tag);

    let doc = docs.import(ticket).await?;
    let author = docs.author_create().await?; // TODO adjust this

    let entry = AuthorizedUsers {
        namespace_id: doc.id().to_string(),
        authorized_users: vec![client_endpoint.id().to_string()],
    };
    let entry = serde_json::to_vec(&entry)?;

    doc.set_bytes(author, "accesslist", entry).await?;

    conn.close(0u32.into(), b"all done!");

    Ok(())
}

async fn query(
    client_endpoint: &Endpoint,
    server_endpoint: &EndpointId,
    tag: &str,
) -> anyhow::Result<()> {
    let conn = client_endpoint.connect(*server_endpoint, b"query").await?;

    let (mut send, mut recv) = conn.open_bi().await?;

    send.write_all(tag.as_bytes()).await?;
    send.finish()?;

    let mut hash_bytes = [0u8; 32];

    println!("Connection opened");

    loop {
        match recv.read_exact(&mut hash_bytes).await {
            Err(iroh::endpoint::ReadExactError::FinishedEarly(_)) => break,
            Err(err) => return Err(err.into()),
            Ok(_) => {}
        };
        println!("{:?}", hash_bytes)
    }
    conn.close(0u32.into(), b"done");

    Ok(())
}

pub async fn add_ticket(docs: &Docs, doc_ticket: &str, blobs: &MemStore) -> anyhow::Result<()> {
    let doc_ticket = DocTicket::from_str(doc_ticket.trim())?;
    let doc = docs.import(doc_ticket).await?;
    let blobs = blobs.clone();

    tokio::spawn(async move {
        let mut events = doc.subscribe().await.unwrap();
        while let Some(event) = events.next().await {
            match event.unwrap() {
                LiveEvent::InsertRemote { entry, .. } => {
                    println!("peer inserted {:?}", entry.key());
                }
                LiveEvent::ContentReady { hash } => {
                    println!("content {hash} is now available locally");

                    if let Ok(content) = blobs.get_bytes(hash).await {
                        let authorized_users: AuthorizedUsers =
                            serde_json::from_slice(&content).unwrap();

                        println!(
                            "Authorized users now: {:?}",
                            authorized_users.authorized_users
                        );
                    };
                }
                LiveEvent::PendingContentReady => {
                    println!("Pending content ready");
                }

                LiveEvent::SyncFinished(event) => {
                    print!("Sync finished: {:?}", event);
                }

                _ => {}
            }
        }
    });

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut input = String::new();
    // println!("Enter endpoint: ");
    // io::stdin()
    //     .read_line(&mut input)
    //     .expect("Failed to read line");

    // let server_id = PublicKey::from_str(input.trim())?;

    // stuff for docs
    let client_endpoint = Endpoint::bind(presets::N0).await?;
    let blobs = MemStore::default();
    let gossip = Gossip::builder().spawn(client_endpoint.clone());
    let docs = Docs::memory()
        .spawn(client_endpoint.clone(), (*blobs).clone(), gossip.clone())
        .await?;

    // register all three protocols on the router
    let router = Router::builder(client_endpoint.clone())
        .accept(iroh_blobs::ALPN, BlobsProtocol::new(&blobs, None))
        .accept(iroh_gossip::ALPN, gossip)
        .accept(iroh_docs::ALPN, docs.clone())
        .spawn();

    println!("Our endpoint id: {}", client_endpoint.id());

    // if let Err(e) = upload(&client_endpoint, &server_id, docs.clone()).await {
    //     eprintln!("Failed to send client video! {}", e)
    // };

    loop {
        input.clear();
        println!("Receive ticket");
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");

        // if let Err(e) = query(&client_endpoint, &server_id, input.trim()).await {
        //     eprintln!("Failed to query server! {}", e)
        // };
        if let Err(e) = add_ticket(&docs, &input, &blobs).await {
            eprintln!("Failed to import doc: {}", e)
        }
    }

    // tokio::signal::ctrl_c().await?;
    // router.shutdown().await?;
    // Ok(())
}
