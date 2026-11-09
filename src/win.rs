//! Win32 helpers: GBK conversion, local time, memory stats, file dialogs, URLs.

#[cfg(windows)]
mod imp {
    use std::path::PathBuf;
    use windows_sys::Win32::Foundation::{HWND, SYSTEMTIME};
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, WideCharToMultiByte};
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY,
        OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn decode_gbk(bytes: &[u8]) -> String {
        if bytes.is_empty() {
            return String::new();
        }
        unsafe {
            let n = MultiByteToWideChar(936, 0, bytes.as_ptr(), bytes.len() as i32, std::ptr::null_mut(), 0);
            if n <= 0 {
                return String::from_utf8_lossy(bytes).into_owned();
            }
            let mut buf = vec![0u16; n as usize];
            let got = MultiByteToWideChar(
                936, 0, bytes.as_ptr(), bytes.len() as i32, buf.as_mut_ptr(), n,
            );
            buf.truncate(got.max(0) as usize);
            String::from_utf16_lossy(&buf)
        }
    }

    pub fn encode_gbk(text: &str) -> Vec<u8> {
        let w: Vec<u16> = text.encode_utf16().collect();
        if w.is_empty() {
            return Vec::new();
        }
        unsafe {
            let n = WideCharToMultiByte(
                936, 0, w.as_ptr(), w.len() as i32, std::ptr::null_mut(), 0,
                std::ptr::null(), std::ptr::null_mut(),
            );
            if n <= 0 {
                return text.as_bytes().to_vec();
            }
            let mut buf = vec![0u8; n as usize];
            let got = WideCharToMultiByte(
                936, 0, w.as_ptr(), w.len() as i32, buf.as_mut_ptr(), n,
                std::ptr::null(), std::ptr::null_mut(),
            );
            buf.truncate(got.max(0) as usize);
            buf
        }
    }

    pub fn local_time() -> Option<(u16, u16, u16, u16)> {
        unsafe {
            let mut st: SYSTEMTIME = std::mem::zeroed();
            GetLocalTime(&mut st);
            if st.wYear == 0 {
                None
            } else {
                Some((st.wHour, st.wMinute, st.wSecond, st.wMilliseconds))
            }
        }
    }

    #[repr(C)]
    struct PmcEx2 {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
        private_working_set_size: usize,
        shared_commit_usage: usize,
    }

    /// (private working set MB, private commit MB)
    pub fn memory_mb() -> (f32, f32) {
        unsafe {
            let mut c: PmcEx2 = std::mem::zeroed();
            c.cb = std::mem::size_of::<PmcEx2>() as u32;
            let size = std::mem::size_of::<PmcEx2>() as u32;
            let ok = GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                size,
            );
            if ok == 0 {
                return (0.0, 0.0);
            }
            (
                c.private_working_set_size as f32 / (1024.0 * 1024.0),
                c.private_usage as f32 / (1024.0 * 1024.0),
            )
        }
    }

    pub fn open_url(url: &str) {
        let op = wide("open");
        let u = wide(url);
        unsafe {
            ShellExecuteW(
                0 as HWND, op.as_ptr(), u.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL,
            );
        }
    }

    fn dialog(
        title: &str,
        filter: &[(&str, &str)],
        initial: &str,
        save: bool,
        ext: &str,
    ) -> Option<PathBuf> {
        let mut f: Vec<u16> = Vec::new();
        for (name, pat) in filter {
            f.extend(name.encode_utf16());
            f.push(0);
            f.extend(pat.encode_utf16());
            f.push(0);
        }
        f.push(0);
        let mut file = vec![0u16; 2048];
        for (i, c) in initial.encode_utf16().take(file.len() - 1).enumerate() {
            file[i] = c;
        }
        let title_w = wide(title);
        let ext_w = wide(ext);
        let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = 0 as HWND;
        ofn.lpstrFilter = f.as_ptr();
        ofn.nFilterIndex = 1;
        ofn.lpstrFile = file.as_mut_ptr();
        ofn.nMaxFile = file.len() as u32;
        ofn.lpstrTitle = title_w.as_ptr();
        ofn.lpstrDefExt = ext_w.as_ptr();
        ofn.Flags = OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_HIDEREADONLY;
        if save {
            ofn.Flags |= OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST;
        } else {
            ofn.Flags |= OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
        }
        let ok = unsafe {
            if save {
                GetSaveFileNameW(&mut ofn)
            } else {
                GetOpenFileNameW(&mut ofn)
            }
        };
        if ok == 0 {
            return None;
        }
        let end = file.iter().position(|&c| c == 0).unwrap_or(file.len());
        let s = String::from_utf16_lossy(&file[..end]);
        if s.is_empty() { None } else { Some(PathBuf::from(s)) }
    }

    pub fn open_file_dialog(title: &str, filter: &[(&str, &str)], initial: &str) -> Option<PathBuf> {
        dialog(title, filter, initial, false, "")
    }

    pub fn save_file_dialog(
        title: &str,
        filter: &[(&str, &str)],
        initial: &str,
        ext: &str,
    ) -> Option<PathBuf> {
        dialog(title, filter, initial, true, ext)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::PathBuf;
    pub fn decode_gbk(b: &[u8]) -> String {
        encoding_rs::GBK.decode(b).0.into_owned()
    }
    pub fn encode_gbk(s: &str) -> Vec<u8> {
        encoding_rs::GBK.encode(s).0.into_owned()
    }
    pub fn local_time() -> Option<(u16, u16, u16, u16)> {
        None
    }
    pub fn memory_mb() -> (f32, f32) {
        (0.0, 0.0)
    }
    pub fn open_url(url: &str) {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(opener).arg(url).spawn();
    }
    pub fn open_file_dialog(_t: &str, _f: &[(&str, &str)], _i: &str) -> Option<PathBuf> {
        None
    }
    /// No native dialog without an extra toolkit: fall back to the current
    /// directory so saving still works on Linux/macOS.
    pub fn save_file_dialog(
        _t: &str,
        _f: &[(&str, &str)],
        initial: &str,
        ext: &str,
    ) -> Option<PathBuf> {
        if !initial.is_empty() {
            return Some(PathBuf::from(initial));
        }
        let name = if ext.is_empty() {
            "CXCOM_output".to_string()
        } else {
            format!("CXCOM_output.{ext}")
        };
        Some(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(name))
    }
}

pub use imp::*;
