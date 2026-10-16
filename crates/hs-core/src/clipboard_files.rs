//! Files and folders on the system clipboard, so a file pane can take what
//! was copied in the file manager and give back something the file manager
//! can paste.
//!
//! Windows carries a file list as `CF_HDROP`: a `DROPFILES` header followed
//! by UTF-16 paths, each ended with a NUL and the list ended with another.
//! The encoding and decoding of that block are plain functions with tests;
//! only the clipboard calls around them are platform code.
//!
//! Elsewhere there is no agreed format a file manager will read back, so
//! [`write`] says so rather than pretending.

use std::path::PathBuf;

/// Bytes of a `DROPFILES` block for these paths, ready to go on the
/// clipboard as `CF_HDROP`.
pub fn encode_drop_files(paths: &[PathBuf]) -> Vec<u8> {
    // DROPFILES: offset to the list, a point, and two flags. 20 bytes.
    const HEADER: u32 = 20;
    let mut out = Vec::with_capacity(HEADER as usize + paths.len() * 64);
    out.extend_from_slice(&HEADER.to_le_bytes()); // pFiles
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0i32.to_le_bytes()); // fNC
    out.extend_from_slice(&1i32.to_le_bytes()); // fWide: the paths are UTF-16
    for p in paths {
        for unit in p.as_os_str().to_string_lossy().encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    // The list itself ends with an empty string.
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Read the double-NUL-terminated UTF-16 list a `DROPFILES` block carries.
pub fn decode_file_list(units: &[u16]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, u) in units.iter().enumerate() {
        if *u != 0 {
            continue;
        }
        if i > start {
            out.push(PathBuf::from(String::from_utf16_lossy(&units[start..i])));
        }
        start = i + 1;
        // Two NULs in a row end the list.
        if units.get(start) == Some(&0) {
            break;
        }
    }
    out
}

#[cfg(windows)]
mod sys {
    use super::*;
    use windows_sys::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows_sys::Win32::System::Ole::CF_HDROP;

    /// The clipboard is a shared resource: open it, do one thing, close it.
    struct Clipboard;

    impl Clipboard {
        fn open() -> Option<Clipboard> {
            // A few tries: another program may hold it for a moment.
            for _ in 0..5 {
                if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                    return Some(Clipboard);
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            None
        }
    }

    impl Drop for Clipboard {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    pub fn has_files() -> bool {
        unsafe { IsClipboardFormatAvailable(CF_HDROP as u32) != 0 }
    }

    pub fn read() -> Vec<PathBuf> {
        if !has_files() {
            return Vec::new();
        }
        let Some(_guard) = Clipboard::open() else {
            return Vec::new();
        };
        unsafe {
            let handle = GetClipboardData(CF_HDROP as u32);
            if handle.is_null() {
                return Vec::new();
            }
            let ptr = GlobalLock(handle as HGLOBAL) as *const u8;
            if ptr.is_null() {
                return Vec::new();
            }
            // Past the DROPFILES header lies the UTF-16 list.
            let offset = std::ptr::read_unaligned(ptr as *const u32) as usize;
            let wide = ptr.add(offset) as *const u16;
            // The list ends with two NULs; walk to find how long it is.
            let mut len = 0usize;
            loop {
                let a = std::ptr::read_unaligned(wide.add(len));
                let b = std::ptr::read_unaligned(wide.add(len + 1));
                len += 1;
                if a == 0 && b == 0 {
                    break;
                }
                if len > 1 << 20 {
                    break;
                }
            }
            let units = std::slice::from_raw_parts(wide, len + 1);
            let paths = decode_file_list(units);
            GlobalUnlock(handle as HGLOBAL);
            paths
        }
    }

    pub fn write(paths: &[PathBuf]) -> Result<(), String> {
        if paths.is_empty() {
            return Err("nothing to copy".into());
        }
        let bytes = encode_drop_files(paths);
        let Some(_guard) = Clipboard::open() else {
            return Err("another program is holding the clipboard".into());
        };
        unsafe {
            if EmptyClipboard() == 0 {
                return Err("the clipboard refused to be emptied".into());
            }
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if handle.is_null() {
                return Err("out of memory for the clipboard".into());
            }
            let ptr = GlobalLock(handle) as *mut u8;
            if ptr.is_null() {
                return Err("the clipboard block could not be locked".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            GlobalUnlock(handle);
            // The clipboard owns the block from here on.
            if SetClipboardData(CF_HDROP as u32, handle as HANDLE).is_null() {
                return Err("the clipboard refused the files".into());
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod sys {
    use super::*;

    pub fn has_files() -> bool {
        false
    }

    pub fn read() -> Vec<PathBuf> {
        Vec::new()
    }

    pub fn write(_paths: &[PathBuf]) -> Result<(), String> {
        Err("copying files to the clipboard is implemented on Windows only".into())
    }
}

/// Whether the clipboard currently holds files.
pub fn has_files() -> bool {
    sys::has_files()
}

/// The files on the clipboard, empty when there are none.
pub fn read() -> Vec<PathBuf> {
    sys::read()
}

/// Put files on the clipboard, so the file manager can paste them.
pub fn write(paths: &[PathBuf]) -> Result<(), String> {
    sys::write(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_starts_with_its_header_and_ends_with_two_nuls() {
        let bytes = encode_drop_files(&[PathBuf::from("C:\\a.txt")]);
        assert_eq!(&bytes[0..4], &20u32.to_le_bytes());
        // fWide is set: the paths are UTF-16.
        assert_eq!(&bytes[16..20], &1i32.to_le_bytes());
        assert_eq!(&bytes[bytes.len() - 4..], &[0, 0, 0, 0]);
    }

    #[test]
    fn paths_round_trip_through_the_block() {
        let paths = vec![
            PathBuf::from("C:\\one.txt"),
            PathBuf::from("C:\\a folder\\two.txt"),
        ];
        let bytes = encode_drop_files(&paths);
        // Read the list back out of the block, as the clipboard would.
        let units: Vec<u16> = bytes[20..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        assert_eq!(decode_file_list(&units), paths);
    }

    #[test]
    fn a_list_ends_at_the_empty_string() {
        // "a\0b\0\0" is two paths; anything after the double NUL is not ours.
        let units: Vec<u16> = "a\0b\0\0c\0\0".encode_utf16().collect();
        assert_eq!(
            decode_file_list(&units),
            vec![PathBuf::from("a"), PathBuf::from("b")]
        );
        assert!(decode_file_list(&[0, 0]).is_empty());
        assert!(decode_file_list(&[]).is_empty());
    }

    #[test]
    fn unicode_survives_the_trip() {
        let paths = vec![PathBuf::from("C:\\naïve\\日本.txt")];
        let bytes = encode_drop_files(&paths);
        let units: Vec<u16> = bytes[20..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        assert_eq!(decode_file_list(&units), paths);
    }
}
