//! Files dragged out of a window to Explorer or another application: an
//! OLE drag of a data object that holds them as `CF_HDROP`.
use std::mem::ManuallyDrop;
use std::path::PathBuf;

use windows::Win32::Foundation::{
    DATA_S_SAMEFORMATETC, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS,
    DV_E_FORMATETC, E_NOTIMPL, E_OUTOFMEMORY, GlobalFree, OLE_E_ADVISENOTSUPPORTED, S_OK,
};
use windows::Win32::System::Com::{
    DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl,
    IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, DoDragDrop, IDropSource,
    IDropSource_Impl,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::SHCreateStdEnumFmtEtc;
use windows::core::{BOOL, HRESULT, Ref, implement};

/// A `DROPFILES` header (wide names) followed by each path, nul-terminated,
/// and a closing nul: what `CF_HDROP` holds.
pub fn drop_files(paths: &[PathBuf]) -> Vec<u8> {
    // `pFiles`, `pt` (two `LONG`s), `fNC`, `fWide`.
    let mut bytes = Vec::new();
    bytes.extend(20u32.to_le_bytes());
    bytes.extend([0u8; 8]);
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    for path in paths {
        for unit in path.as_os_str().to_string_lossy().encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        bytes.extend([0, 0]);
    }
    bytes.extend([0, 0]);
    bytes
}

fn file_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn is_file_format(format: *const FORMATETC) -> bool {
    // SAFETY: OLE passes a valid format or null.
    let Some(format) = (unsafe { format.as_ref() }) else {
        return false;
    };
    format.cfFormat == CF_HDROP.0
        && format.dwAspect == DVASPECT_CONTENT.0
        && format.tymed & TYMED_HGLOBAL.0 as u32 != 0
}

#[implement(IDataObject)]
struct FileData(Vec<u8>);

#[allow(non_snake_case)]
impl IDataObject_Impl for FileData_Impl {
    fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        if !is_file_format(format) {
            return Err(DV_E_FORMATETC.into());
        }
        // SAFETY: the memory is allocated, filled while locked and handed to
        // the receiver, who frees it with `ReleaseStgMedium` (no release
        // object is given).
        unsafe {
            let global = GlobalAlloc(GMEM_MOVEABLE, self.0.len())?;
            let target = GlobalLock(global);
            if target.is_null() {
                let _ = GlobalFree(Some(global));
                return Err(E_OUTOFMEMORY.into());
            }
            std::ptr::copy_nonoverlapping(self.0.as_ptr(), target.cast::<u8>(), self.0.len());
            let _ = GlobalUnlock(global);
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: global },
                pUnkForRelease: ManuallyDrop::new(None),
            })
        }
    }

    fn GetDataHere(
        &self,
        _format: *const FORMATETC,
        _medium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
        if is_file_format(format) {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(&self, _format: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
        // SAFETY: OLE passes a writable format or null.
        if let Some(out) = unsafe { out.as_mut() } {
            out.ptd = std::ptr::null_mut();
        }
        DATA_S_SAMEFORMATETC
    }

    fn SetData(
        &self,
        _format: *const FORMATETC,
        _medium: *const STGMEDIUM,
        _release: BOOL,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
        if direction == DATADIR_GET.0 as u32 {
            // SAFETY: one format, copied by the enumerator.
            unsafe { SHCreateStdEnumFmtEtc(&[file_format()]) }
        } else {
            Err(E_NOTIMPL.into())
        }
    }

    fn DAdvise(
        &self,
        _format: *const FORMATETC,
        _flags: u32,
        _sink: Ref<IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

#[implement(IDropSource)]
struct FileSource;

#[allow(non_snake_case)]
impl IDropSource_Impl for FileSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Drags `paths` until they are dropped or the drag is cancelled, running
/// the system's drag loop (it pumps the thread's messages), and returns
/// what the target did with them.
pub(crate) fn drag(paths: &[PathBuf]) -> DROPEFFECT {
    let data: IDataObject = FileData(drop_files(paths)).into();
    let source: IDropSource = FileSource.into();
    let mut effect = DROPEFFECT_NONE;
    // SAFETY: OLE is initialised on this thread (`OleInitialize` when the
    // platform starts); both objects live for the call.
    let _ = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
    effect
}
