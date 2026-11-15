//! What a drag from another application offers besides files (`CF_HDROP`):
//! virtual files (`FileGroupDescriptorW` with `FileContents`, what Outlook,
//! a browser and Photos drag), written into a folder made for the drop; a
//! browser's dragged image as image data (`PNG`, `CF_DIBV5`, `CF_DIB`), its
//! address (`UniformResourceLocatorW`), the page's HTML for it
//! (`HTML Format`) and text.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use gpui::{ExternalDropData, ExternalDropFile, PromisedFile, PromisedFiles};
use windows::Win32::Foundation::S_OK;
use windows::Win32::System::{
    Com::{
        DVASPECT_CONTENT, FORMATETC, IDataObject, IDataObject_Impl, TYMED_HGLOBAL, TYMED_ISTREAM,
    },
    DataExchange::RegisterClipboardFormatW,
    Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT, ReleaseStgMedium},
};
use windows::core::{PCWSTR, implement, w};

/// The most one in-memory file may hold; a larger one is left out.
const FILE_LIMIT: usize = 1 << 30;
/// `FILE_ATTRIBUTE_DIRECTORY`, `FD_ATTRIBUTES` (the attributes field is
/// valid) and `FD_FILESIZE` (the size fields are).
const DIRECTORY: u32 = 0x10;
const FD_ATTRIBUTES: u32 = 0x4;
const FD_FILESIZE: u32 = 0x40;
/// The size of one `FILEDESCRIPTORW`.
const DESCRIPTOR: usize = 592;

fn format(name: PCWSTR) -> u16 {
    // SAFETY: a nul-terminated wide string; 0 (failure) matches nothing.
    unsafe { RegisterClipboardFormatW(name) as u16 }
}

static PNG: LazyLock<u16> = LazyLock::new(|| format(w!("PNG")));
static URL_WIDE: LazyLock<u16> = LazyLock::new(|| format(w!("UniformResourceLocatorW")));
static URL: LazyLock<u16> = LazyLock::new(|| format(w!("UniformResourceLocator")));
static HTML: LazyLock<u16> = LazyLock::new(|| format(w!("HTML Format")));
static FILE_GROUP: LazyLock<u16> = LazyLock::new(|| format(w!("FileGroupDescriptorW")));
static FILE_CONTENTS: LazyLock<u16> = LazyLock::new(|| format(w!("FileContents")));

fn etc(format: u16, lindex: i32, tymed: u32) -> FORMATETC {
    FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex,
        tymed,
    }
}

fn offers(data: &IDataObject, format: u16) -> bool {
    format != 0 && unsafe { data.QueryGetData(&etc(format, -1, TYMED_HGLOBAL.0 as u32)) } == S_OK
}

/// Whether a drag with no files offers anything [`read`] reads.
pub(crate) fn offers_drop_data(data: &IDataObject) -> bool {
    [
        *FILE_GROUP,
        *PNG,
        CF_DIBV5.0,
        CF_DIB.0,
        *URL_WIDE,
        *URL,
        *HTML,
        CF_UNICODETEXT.0,
    ]
    .into_iter()
    .any(|format| offers(data, format))
}

/// Copies the data of `format` (item `lindex`), from global memory or a
/// stream, into `sink`: at most `limit` bytes of it (global memory, which
/// the system may round up, is cut there); `Err` with the reason when it
/// cannot be read or a stream holds more.
fn copy(
    data: &IDataObject,
    format: u16,
    lindex: i32,
    limit: u64,
    sink: &mut dyn Write,
) -> Result<(), String> {
    if format == 0 {
        return Err("format not registered".into());
    }
    let wanted = etc(format, lindex, (TYMED_HGLOBAL.0 | TYMED_ISTREAM.0) as u32);
    // SAFETY: `GetData` hands over a medium the caller releases with
    // `ReleaseStgMedium`, which every path below does once; the union
    // field read is the one `tymed` names.
    unsafe {
        let mut medium = data.GetData(&wanted).map_err(|error| error.message())?;
        let copied = if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            match crate::clipboard::LockedGlobal::lock(medium.u.hGlobal) {
                Some(locked) => {
                    let bytes = locked.as_bytes();
                    let end = bytes
                        .len()
                        .min(usize::try_from(limit).unwrap_or(usize::MAX));
                    sink.write_all(&bytes[..end])
                        .map_err(|error| error.to_string())
                }
                None => Err("the data could not be locked".into()),
            }
        } else if medium.tymed == TYMED_ISTREAM.0 as u32 {
            match (*medium.u.pstm).as_ref() {
                Some(stream) => {
                    let mut chunk = vec![0u8; 1 << 16];
                    let mut total = 0u64;
                    loop {
                        let mut got = 0u32;
                        let result = stream.Read(
                            chunk.as_mut_ptr().cast(),
                            chunk.len() as u32,
                            Some(&mut got as *mut u32),
                        );
                        if result.is_err() {
                            break Err(result.message());
                        }
                        if got == 0 {
                            break Ok(());
                        }
                        total += u64::from(got);
                        if total > limit {
                            break Err("the data is longer than it was said to be".into());
                        }
                        if let Err(error) = sink.write_all(&chunk[..got as usize]) {
                            break Err(error.to_string());
                        }
                    }
                }
                None => Err("no stream".into()),
            }
        } else {
            Err("the data is in a form not read".into())
        };
        ReleaseStgMedium(&mut medium);
        copied
    }
}

/// The bytes of `format` (item `lindex`), at most [`FILE_LIMIT`] of them.
fn bytes(data: &IDataObject, format: u16, lindex: i32) -> Option<Vec<u8>> {
    let mut all = Vec::new();
    copy(data, format, lindex, FILE_LIMIT as u64, &mut all).ok()?;
    Some(all)
}

/// UTF-16 text up to its terminating nul.
fn wide_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Narrow text up to its terminating nul.
fn narrow_text(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// A file a `FILEGROUPDESCRIPTORW` describes.
struct Descriptor {
    name: String,
    directory: bool,
    /// Its size, when the descriptor gives it.
    size: Option<u64>,
}

/// The files a `FILEGROUPDESCRIPTORW` describes, in order.
fn file_group(group: &[u8]) -> Vec<Descriptor> {
    let Some(count) = group
        .get(..4)
        .map(|count| u32::from_le_bytes(count.try_into().unwrap()))
    else {
        return Vec::new();
    };
    let word = |descriptor: &[u8], at: usize| {
        u32::from_le_bytes(descriptor[at..at + 4].try_into().unwrap())
    };
    (0..count as usize)
        .map_while(|index| {
            let descriptor = group.get(4 + index * DESCRIPTOR..4 + (index + 1) * DESCRIPTOR)?;
            let flags = word(descriptor, 0);
            let directory = flags & FD_ATTRIBUTES != 0 && word(descriptor, 36) & DIRECTORY != 0;
            let size = (flags & FD_FILESIZE != 0)
                .then(|| (u64::from(word(descriptor, 64)) << 32) | u64::from(word(descriptor, 68)));
            Some(Descriptor {
                name: wide_text(&descriptor[72..592]),
                directory,
                size,
            })
        })
        .collect()
}

/// The name a virtual file is written under: the last part of the name it
/// was given (a file in a dragged folder names the folder too), without
/// the characters a Windows name cannot hold, never a device's name, and
/// none when nothing is left.
fn virtual_file_name(given: &str) -> Option<String> {
    let last = given.rsplit(['\\', '/']).next().unwrap_or(given);
    let cleaned: String = last
        .chars()
        .filter(|character| !matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .filter(|character| !character.is_control())
        .collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']).trim_start();
    if cleaned.is_empty() {
        return None;
    }
    let stem = cleaned
        .split('.')
        .next()
        .unwrap_or(cleaned)
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    Some(if device {
        format!("_{cleaned}")
    } else {
        cleaned.to_owned()
    })
}

/// `name` in `folder`, or `stem (2).ext` and on when that is taken.
fn unused_path(folder: &Path, name: &str) -> PathBuf {
    let path = folder.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
        _ => (name, None),
    };
    (2..)
        .map(|number| {
            folder.join(match extension {
                Some(extension) => format!("{stem} ({number}).{extension}"),
                None => format!("{stem} ({number})"),
            })
        })
        .find(|path| !path.exists())
        .unwrap()
}

/// A folder no other drop of this process has used, inside `directory`.
fn drop_folder(directory: &Path) -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let folder = directory.join(format!(
        "promised-{now:x}-{:x}-{:x}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// Writes the virtual files `group` describes (folders left out) into a
/// folder made for this drop, while the source still holds them, and
/// reports each: its path, or why it was not written (a file that failed
/// part-way is removed).
fn write_virtual_files(data: &IDataObject, group: &[u8]) -> Option<PromisedFiles> {
    let files: Vec<(usize, Descriptor)> = file_group(group)
        .into_iter()
        .enumerate()
        .filter(|(_, file)| !file.directory)
        .collect();
    if files.is_empty() {
        return None;
    }
    let folder = drop_folder(&gpui::promised_files_directory())
        .map_err(|error| log::error!("no folder for dropped virtual files: {error}"))
        .ok()?;
    let (promised, sender) = PromisedFiles::new(folder.clone(), files.len());
    for (index, file) in files {
        let report: PromisedFile = (|| {
            let name = virtual_file_name(&file.name)
                .ok_or_else(|| format!("no usable name in \"{}\"", file.name))?;
            let path = unused_path(&folder, &name);
            let mut out = std::fs::File::create(&path).map_err(|error| error.to_string())?;
            let limit = file.size.unwrap_or(u64::MAX);
            let written = copy(data, *FILE_CONTENTS, index as i32, limit, &mut out)
                .and_then(|()| out.flush().map_err(|error| error.to_string()))
                .and_then(|()| match (file.size, out.metadata()) {
                    (Some(size), Ok(meta)) if meta.len() != size => {
                        Err("the file is shorter than it was said to be".into())
                    }
                    _ => Ok(()),
                });
            drop(out);
            match written {
                Ok(()) => Ok(path),
                Err(error) => {
                    let _ = std::fs::remove_file(&path);
                    Err(error)
                }
            }
        })();
        let _ = sender.unbounded_send(report);
    }
    Some(promised)
}

/// [`read`], for the application's test of a drop of its own making.
pub fn read_drop_data(data: &IDataObject) -> ExternalDropData {
    read(data)
}

/// The fragment `CF_HTML` marks (`StartFragment`/`EndFragment` byte
/// offsets), else its HTML from `StartHTML`, else all of it.
pub fn html_fragment(cf_html: &str) -> String {
    let offset = |name: &str| -> Option<usize> {
        let at = cf_html.find(name)? + name.len();
        let digits: String = cf_html[at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    let bytes = cf_html.as_bytes();
    let slice = |start: Option<usize>, end: Option<usize>| -> Option<String> {
        let (start, end) = (start?, end.unwrap_or(bytes.len()));
        let text = bytes.get(start..end.min(bytes.len()))?;
        Some(String::from_utf8_lossy(text).into_owned())
    };
    slice(offset("StartFragment:"), offset("EndFragment:"))
        .or_else(|| slice(offset("StartHTML:"), offset("EndHTML:")))
        .unwrap_or_else(|| cf_html.to_owned())
}

/// What `data` offers besides files: its virtual files, written into a
/// folder of the drop's own, one image's data, its address, HTML and text.
/// Nothing of a drag of files.
pub(crate) fn read(data: &IDataObject) -> ExternalDropData {
    let mut dropped = ExternalDropData::default();
    if offers(data, CF_HDROP.0) {
        return dropped;
    }
    if let Some(group) = bytes(data, *FILE_GROUP, -1) {
        dropped.promised = write_virtual_files(data, &group);
    }
    let image = bytes(data, *PNG, -1)
        .map(|png| (png, "image/png"))
        .or_else(|| {
            [CF_DIBV5.0, CF_DIB.0].into_iter().find_map(|format| {
                let dib = bytes(data, format, -1)?;
                Some((crate::clipboard::convert_dib_to_bmp(&dib)?, "image/bmp"))
            })
        });
    if let Some((bytes, media_type)) = image {
        dropped.files.push(ExternalDropFile {
            bytes,
            name: None,
            media_type: Some(media_type.to_owned()),
        });
    }
    let address = bytes(data, *URL_WIDE, -1)
        .map(|bytes| wide_text(&bytes))
        .or_else(|| bytes(data, *URL, -1).map(|bytes| narrow_text(&bytes)));
    dropped
        .uri_lists
        .extend(address.filter(|address| !address.is_empty()));
    if let Some(html) = bytes(data, *HTML, -1) {
        dropped.html.push(html_fragment(&narrow_text(&html)));
    }
    if let Some(text) = bytes(data, CF_UNICODETEXT.0, -1) {
        dropped.text.push(wide_text(&text));
    }
    dropped
}

/// A data object holding virtual files as Outlook and a browser drag them,
/// for the application's test of a drop of its own making: one
/// `FILEGROUPDESCRIPTORW` with each file's name, flags (`FD_ATTRIBUTES`,
/// and `FD_FILESIZE` with the size given) and attributes, and each file's
/// `FileContents` at its index, from global memory or, when asked, a
/// stream. Every other format is refused.
pub fn virtual_files_object(files: Vec<VirtualFile>) -> IDataObject {
    VirtualFiles(files).into()
}

/// A file [`virtual_files_object`] holds.
pub struct VirtualFile {
    pub name: String,
    pub contents: Vec<u8>,
    /// A folder (its contents are never asked for).
    pub directory: bool,
    /// The size its descriptor gives, when it gives one.
    pub size: Option<u64>,
    /// Handed over as a stream rather than global memory.
    pub stream: bool,
}

#[implement(IDataObject)]
struct VirtualFiles(Vec<VirtualFile>);

impl VirtualFiles {
    fn group(&self) -> Vec<u8> {
        let mut group = (self.0.len() as u32).to_le_bytes().to_vec();
        for file in &self.0 {
            let mut descriptor = vec![0u8; DESCRIPTOR];
            let flags = FD_ATTRIBUTES | file.size.map_or(0, |_| FD_FILESIZE);
            descriptor[0..4].copy_from_slice(&flags.to_le_bytes());
            let attributes: u32 = if file.directory { DIRECTORY } else { 0x80 };
            descriptor[36..40].copy_from_slice(&attributes.to_le_bytes());
            let size = file.size.unwrap_or(0);
            descriptor[64..68].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
            descriptor[68..72].copy_from_slice(&(size as u32).to_le_bytes());
            for (index, unit) in file.name.encode_utf16().take(259).enumerate() {
                descriptor[72 + index * 2..74 + index * 2].copy_from_slice(&unit.to_le_bytes());
            }
            group.extend(descriptor);
        }
        group
    }
}

fn global_medium(bytes: &[u8]) -> windows::core::Result<windows::Win32::System::Com::STGMEDIUM> {
    use windows::Win32::System::Com::{STGMEDIUM, STGMEDIUM_0};
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    // SAFETY: the memory is allocated, filled while locked and handed to the
    // receiver, who frees it with `ReleaseStgMedium`.
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))?;
        let target = GlobalLock(global);
        if target.is_null() {
            return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(global);
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: global },
            pUnkForRelease: std::mem::ManuallyDrop::new(None),
        })
    }
}

#[allow(non_snake_case)]
impl IDataObject_Impl for VirtualFiles_Impl {
    fn GetData(
        &self,
        format: *const FORMATETC,
    ) -> windows::core::Result<windows::Win32::System::Com::STGMEDIUM> {
        use windows::Win32::Foundation::DV_E_FORMATETC;
        use windows::Win32::System::Com::{STGMEDIUM, STGMEDIUM_0};
        // SAFETY: OLE passes a valid format or null.
        let Some(format) = (unsafe { format.as_ref() }) else {
            return Err(DV_E_FORMATETC.into());
        };
        if format.cfFormat == *FILE_GROUP && format.tymed & TYMED_HGLOBAL.0 as u32 != 0 {
            return global_medium(&self.group());
        }
        let file = usize::try_from(format.lindex)
            .ok()
            .and_then(|index| self.0.get(index))
            .filter(|file| !file.directory);
        let (true, Some(file)) = (format.cfFormat == *FILE_CONTENTS, file) else {
            return Err(DV_E_FORMATETC.into());
        };
        if file.stream && format.tymed & TYMED_ISTREAM.0 as u32 != 0 {
            // SAFETY: a stream over a copy of the bytes, handed to the
            // receiver, who releases it with `ReleaseStgMedium`.
            let stream = unsafe {
                windows::Win32::UI::Shell::SHCreateMemStream(Some(file.contents.as_slice()))
            }
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY))?;
            return Ok(STGMEDIUM {
                tymed: TYMED_ISTREAM.0 as u32,
                u: STGMEDIUM_0 {
                    pstm: std::mem::ManuallyDrop::new(Some(stream)),
                },
                pUnkForRelease: std::mem::ManuallyDrop::new(None),
            });
        }
        if format.tymed & TYMED_HGLOBAL.0 as u32 != 0 {
            return global_medium(&file.contents);
        }
        Err(DV_E_FORMATETC.into())
    }

    fn GetDataHere(
        &self,
        _format: *const FORMATETC,
        _medium: *mut windows::Win32::System::Com::STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }

    fn QueryGetData(&self, format: *const FORMATETC) -> windows::core::HRESULT {
        // SAFETY: OLE passes a valid format or null.
        let offered = unsafe { format.as_ref() }.is_some_and(|format| {
            format.cfFormat == *FILE_GROUP || format.cfFormat == *FILE_CONTENTS
        });
        if offered {
            S_OK
        } else {
            windows::Win32::Foundation::DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _format: *const FORMATETC,
        out: *mut FORMATETC,
    ) -> windows::core::HRESULT {
        // SAFETY: OLE passes a writable format or null.
        if let Some(out) = unsafe { out.as_mut() } {
            out.ptd = std::ptr::null_mut();
        }
        windows::Win32::Foundation::DATA_S_SAMEFORMATETC
    }

    fn SetData(
        &self,
        _format: *const FORMATETC,
        _medium: *const windows::Win32::System::Com::STGMEDIUM,
        _release: windows::core::BOOL,
    ) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }

    fn EnumFormatEtc(
        &self,
        _direction: u32,
    ) -> windows::core::Result<windows::Win32::System::Com::IEnumFORMATETC> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }

    fn DAdvise(
        &self,
        _format: *const FORMATETC,
        _flags: u32,
        _sink: windows::core::Ref<windows::Win32::System::Com::IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<windows::Win32::System::Com::IEnumSTATDATA> {
        Err(windows::Win32::Foundation::OLE_E_ADVISENOTSUPPORTED.into())
    }
}
