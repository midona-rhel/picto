//! Portable document preview worker. No UI toolkit or OS document APIs.
use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct Pages {
    pub paths: Vec<PathBuf>,
    _directory: tempfile::TempDir,
}

fn run(command: &mut Command) -> Result<()> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().context("Document preview requires LibreOffice and Poppler on PATH (or PICTO_SOFFICE / PICTO_PDFTOPPM)")?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("Document renderer exited with {status}");
            }
            return Ok(());
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Document rendering exceeded 60 seconds");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub fn render(path: &Path) -> Result<Pages> {
    anyhow::ensure!(
        std::fs::metadata(path)?.len() <= 64 * 1024 * 1024,
        "Document exceeds the prototype's 64 MiB limit"
    );
    let directory = tempfile::tempdir()?;
    let input = directory.path().join("document.docx");
    std::fs::copy(path, &input)?;
    let profile = directory.path().join("profile");
    std::fs::create_dir_all(profile.join("user"))?;
    // Disable macros and automatic external-link updates in the isolated profile.
    std::fs::write(
        profile.join("user/registrymodifications.xcu"),
        r#"<?xml version="1.0"?><oor:items xmlns:oor="http://openoffice.org/2001/registry"><item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>3</value></prop></item><item oor:path="/org.openoffice.Office.Writer/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>2</value></prop></item></oor:items>"#,
    )?;
    let profile_url = url::Url::from_directory_path(&profile)
        .map_err(|_| anyhow::anyhow!("Invalid document profile path"))?;
    run(
        Command::new(std::env::var_os("PICTO_SOFFICE").unwrap_or_else(|| "soffice".into()))
            .arg(format!("-env:UserInstallation={profile_url}"))
            .args([
                "--headless",
                "--norestore",
                "--convert-to",
                "pdf:writer_pdf_Export",
                "--outdir",
            ])
            .arg(directory.path())
            .arg(&input),
    )?;
    let pdf = directory.path().join("document.pdf");
    anyhow::ensure!(pdf.is_file(), "Word conversion produced no PDF");
    run(
        Command::new(std::env::var_os("PICTO_PDFTOPPM").unwrap_or_else(|| "pdftoppm".into()))
            .args(["-scale-to", "1600", "-png"])
            .arg(pdf)
            .arg(directory.path().join("page")),
    )?;
    let mut paths = std::fs::read_dir(directory.path())?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect::<Vec<_>>();
    paths.sort_by_key(|p| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("page-"))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0)
    });
    anyhow::ensure!(!paths.is_empty(), "Document renderer produced no pages");
    Ok(Pages {
        paths,
        _directory: directory,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "Requires PICTO_TEST_DOCX, LibreOffice and Poppler"]
    fn real_word_document_renders_and_cleans_up() {
        let pages = super::render(std::path::Path::new(
            &std::env::var("PICTO_TEST_DOCX").unwrap(),
        ))
        .unwrap();
        assert!(pages.paths.len() >= 2);
        let path = pages.paths[0].clone();
        let image = image::open(&path).unwrap();
        assert!(image.width() > 500 && image.height() > 500);
        drop(pages);
        assert!(!path.exists());
    }
}
