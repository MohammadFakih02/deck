//! "What's using this file?": asks Windows' Restart Manager which programs
//! hold a file (or any file in a folder) open.

use serde::Serialize;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, WIN32_ERROR};
use windows::Win32::System::RestartManager::{
    RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY, RM_PROCESS_INFO,
};

#[derive(Serialize, Clone, Debug)]
pub struct Holder {
    pub pid: u32,
    pub name: String,
    pub exe: Option<String>,
}

fn files_under(path: &str, limit: usize) -> Vec<String> {
    let p = std::path::Path::new(path);
    if p.is_file() {
        return vec![path.to_string()];
    }
    jwalk::WalkDir::new(p)
        .skip_hidden(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .take(limit)
        .map(|e| e.path().to_string_lossy().to_string())
        .collect()
}

pub fn who_uses(path: &str) -> Result<Vec<Holder>, String> {
    let files = files_under(path, 4000);
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let mut out: Vec<Holder> = Vec::new();
    unsafe {
        let mut session = 0u32;
        let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
        let r = RmStartSession(&mut session, None, PWSTR(key.as_mut_ptr()));
        if r != WIN32_ERROR(0) {
            return Err(format!("Restart Manager unavailable ({})", r.0));
        }
        for chunk in files.chunks(500) {
            let wides: Vec<Vec<u16>> = chunk.iter().map(|f| crate::util::wide(f)).collect();
            let ptrs: Vec<PCWSTR> = wides.iter().map(|w| PCWSTR(w.as_ptr())).collect();
            let _ = RmRegisterResources(session, Some(&ptrs), None, None);
        }
        let mut needed = 0u32;
        let mut count = 0u32;
        let mut reasons = 0u32;
        let mut buf: Vec<RM_PROCESS_INFO> = Vec::new();
        let mut r = RmGetList(session, &mut needed, &mut count, None, &mut reasons);
        if r == ERROR_MORE_DATA {
            buf = vec![RM_PROCESS_INFO::default(); needed as usize + 4];
            count = buf.len() as u32;
            r = RmGetList(session, &mut needed, &mut count, Some(buf.as_mut_ptr()), &mut reasons);
        }
        let _ = RmEndSession(session);
        if r != WIN32_ERROR(0) {
            return Err(format!("Couldn't check ({})", r.0));
        }
        for p in buf.iter().take(count as usize) {
            let pid = p.Process.dwProcessId;
            if out.iter().any(|h| h.pid == pid) {
                continue;
            }
            let exe = crate::util::process_path(pid);
            out.push(Holder { pid, name: crate::util::from_wide(&p.strAppName), exe });
        }
    }
    Ok(out)
}
