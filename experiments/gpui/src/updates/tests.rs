use super::feed::FeedFile;
use super::{fetch_installer, ready_installer};
use base64::Engine as _;
use sha2::Digest as _;
use std::io::{Read as _, Write as _};

/// Starts a web server on the loopback address that answers each of the
/// first `count` connections with `body`. Returns the address to request.
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
        admin: false,
    }
}

/// A download that is not the file the feed describes, because its SHA-512
/// or its size differs, ends with an error and leaves no file in the folder,
/// so it can never be offered for installing. A download that matches is
/// kept under the file name the feed gives. The download, the check and the
/// file handling are the same code on every system, so this test runs on
/// all of them.
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

    // The server sends only the first 1000 bytes, so the download is shorter
    // than the size the feed gives.
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

/// Restart and Install, and the installation that runs when the app quits,
/// start an installer only when it is in this app's `updates/` folder, has
/// the file name the feed gives, and still has the SHA-512 and the size the
/// feed gives at the moment it is started. A file that was changed after
/// its download, a file in another folder, a file with another name and a
/// link in the file's place are all refused.
#[test]
fn only_a_verified_installer_in_the_updates_folder_is_started() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("updates");
    std::fs::create_dir_all(&folder).unwrap();
    let installer: Vec<u8> = (0..50_000_u32).map(|index| (index % 239) as u8).collect();
    let file = described(&installer);
    let path = folder.join("Picto-Setup-9.9.9.exe");
    std::fs::write(&path, &installer).unwrap();
    assert_eq!(ready_installer(&path, &folder, &file).unwrap(), path);

    // The file is changed on disk after it was verified: first one byte of
    // it, then its length.
    let mut changed = installer.clone();
    changed[10] ^= 1;
    std::fs::write(&path, &changed).unwrap();
    assert!(ready_installer(&path, &folder, &file).is_err());
    let mut longer = installer.clone();
    longer.push(0);
    std::fs::write(&path, &longer).unwrap();
    assert!(ready_installer(&path, &folder, &file).is_err());

    // A file with the right bytes is refused when it is outside the updates
    // folder, and when it does not have the file name the feed gives.
    let elsewhere = root.path().join("Picto-Setup-9.9.9.exe");
    std::fs::write(&elsewhere, &installer).unwrap();
    assert!(ready_installer(&elsewhere, &folder, &file).is_err());
    let renamed = folder.join("other.exe");
    std::fs::write(&renamed, &installer).unwrap();
    assert!(ready_installer(&renamed, &folder, &file).is_err());

    // A symbolic link in the updates folder that points to the right file in
    // another folder is refused.
    #[cfg(unix)]
    {
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        assert!(ready_installer(&path, &folder, &file).is_err());
    }
}

/// Checks an update feed that `package.sh` wrote, as the update check uses
/// it. The environment variable `PICTO_GPUI_TEST_FEED` names the feed
/// (`latest.yml`, `latest-linux.yml` or `latest-mac.yml`), and the packages
/// it names are in the same folder. The feed must be readable, must name
/// this build's version, and every package must have the SHA-512 and the
/// size that the feed gives for it. A Windows feed must name the installer
/// the updater downloads on Windows, and a Linux feed the AppImage it
/// downloads on Linux.
#[test]
#[ignore = "needs PICTO_GPUI_TEST_FEED, the path of a feed that package.sh wrote"]
fn a_feed_written_by_the_package_script_describes_its_packages() {
    let path = std::path::PathBuf::from(
        std::env::var_os("PICTO_GPUI_TEST_FEED").expect("PICTO_GPUI_TEST_FEED is set"),
    );
    let text = std::fs::read_to_string(&path).expect("read the feed");
    let feed = super::feed::parse_feed(&text).expect("the feed is readable");
    assert_eq!(feed.version, super::VERSION);
    assert!(!feed.files.is_empty(), "the feed names no package");
    let folder = path.parent().expect("the feed is in a folder");
    for file in &feed.files {
        let name = file.name().expect("a package has a plain file name");
        let bytes = std::fs::read(folder.join(&name)).expect("read the package");
        assert!(file.size.is_some(), "the feed gives no size for {name}");
        super::feed::verify(&sha2::Sha512::digest(&bytes), bytes.len() as u64, file)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
    }
    let platform = match path.file_name().and_then(|name| name.to_str()) {
        Some("latest.yml") => Some("win32"),
        Some("latest-linux.yml") => Some("linux"),
        _ => None,
    };
    if let Some(platform) = platform {
        assert!(
            super::feed::installer(&feed, platform).is_some(),
            "the feed names no installer for {platform}"
        );
    }
}
