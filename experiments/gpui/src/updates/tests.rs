use super::feed::FeedFile;
use super::fetch_installer;
use base64::Engine as _;
use sha2::Digest as _;
use std::io::{Read as _, Write as _};

/// Serves `body` once per connection on the loopback, for as many
/// connections as `count`.
fn serve(body: Vec<u8>, count: usize) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/installer.exe", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().take(count) {
            let mut stream = stream.unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    address
}

fn described(bytes: &[u8]) -> FeedFile {
    FeedFile {
        url: "Picto-Setup-9.9.9.exe".into(),
        sha512: base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(bytes)),
        size: Some(bytes.len() as u64),
    }
}

/// An installer that is not the file the feed describes (another checksum,
/// or another size) is never kept, so it can never be offered; the one that
/// is, is kept under the feed's name. On every system: the download, the
/// check and the file handling are the same code everywhere.
#[test]
fn only_an_installer_matching_the_feed_is_kept() {
    let folder = tempfile::tempdir().unwrap();
    let installer: Vec<u8> = (0..200_000_u32).map(|index| (index % 251) as u8).collect();
    let (sender, _progress) = futures::channel::mpsc::unbounded();

    // The bytes served are not the ones the feed describes.
    let tampered = {
        let mut bytes = installer.clone();
        bytes[1000] ^= 1;
        bytes
    };
    let address = serve(tampered, 1);
    let outcome = crate::net::block_on(fetch_installer(
        &address,
        folder.path(),
        &described(&installer),
        sender.clone(),
    ))
    .unwrap();
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);

    // Cut short: the size the feed gives is not reached.
    let address = serve(installer[..1000].to_vec(), 1);
    let outcome = crate::net::block_on(fetch_installer(
        &address,
        folder.path(),
        &described(&installer),
        sender.clone(),
    ))
    .unwrap();
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);

    let address = serve(installer.clone(), 1);
    let kept = crate::net::block_on(fetch_installer(
        &address,
        folder.path(),
        &described(&installer),
        sender,
    ))
    .unwrap()
    .unwrap();
    assert_eq!(kept, folder.path().join("Picto-Setup-9.9.9.exe"));
    assert_eq!(std::fs::read(&kept).unwrap(), installer);
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
}
