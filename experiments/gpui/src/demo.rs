//! Bundled, non-private samples exercise the same media surface as library files.
pub fn fixtures() -> anyhow::Result<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    for (name, data) in [
        (
            "vector.svg",
            include_bytes!("../assets/fixtures/vector.svg").as_slice(),
        ),
        (
            "interactive.swf",
            include_bytes!("../assets/fixtures/interactive.swf").as_slice(),
        ),
        (
            "document.docx",
            include_bytes!("../assets/fixtures/document.docx").as_slice(),
        ),
    ] {
        std::fs::write(directory.path().join(name), data)?;
    }
    Ok(directory)
}
