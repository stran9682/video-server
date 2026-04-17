use rand::rngs::OsRng;
use willow25::prelude::*;
fn main() {
    let mut csprng = OsRng;

    // the conceptual place where items are stored
    let (namespace_id, namespace_secret) = randomly_generate_communal_namespace(&mut csprng);

    // generate this when Alfie connects to server
    let (alfie_id, alfie_secret) = randomly_generate_subspace(&mut csprng);

    // this is used to sign entries and verify they were made by Alfie
    let communal_cap = WriteCapability::new_communal(namespace_id.clone(), alfie_id.clone());

    let alfie_entry = Entry::builder()
        .namespace_id(namespace_id.clone())
        .subspace_id(alfie_id.clone())
        .path(path!("/ideas"))
        .timestamp(1)
        .payload(b"alfie's entry")
        .build()
        .unwrap();

    // this confirms that Alfie's entry was written by her
    // if it fails, that means there's a fraud!
    let authorized_entry = alfie_entry.into_authorised_entry(&communal_cap, &alfie_secret).unwrap();

    // Now alfie wants to share her subspace with betty
    let mut read_cap = ReadCapability::new_communal(namespace_id.clone(), alfie_id.clone());

    let (betty_id, betty_secret) = randomly_generate_subspace(&mut csprng);

    // this allows betty to have access to only the path /ideas in Alfie's subspace
    read_cap.delegate(
        &alfie_secret,
        Area::new(
            Some(alfie_id.clone()),
            path!("/ideas"),
            TimeRange::Open(0.into()),
        ),
        betty_id.clone(),
    );

    read_cap.receiver();


    // nice try betty!
    let restricted_access = read_cap.includes_area(&Area::new(
        Some(alfie_id),
        path!("/restricted"),
        TimeRange::Open(0.into()),
    ));

    println!("{}", restricted_access)
}
