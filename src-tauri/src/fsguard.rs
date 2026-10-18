//! File work Deck does with admin rights inside folders the signed-in user controls (Temp and the
//! other cleanup folders, Downloads, the Claude folder, received files). Any folder on the way
//! could be swapped for a link to somewhere else (System32) between Deck finding a file and acting
//! on it. So a folder is opened once, checked by its handle to really be inside the intended root,
//! and held (it can't be moved while open); files are then created, deleted and renamed relative
//! to that handle, and never through a path that could be resolved again.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{NtCreateFile, NtSetInformationFile, FileRenameInformation, FileRenameInformationEx, FILE_RENAME_INFORMATION, FILE_RENAME_POSIX_SEMANTICS, FILE_RENAME_REPLACE_IF_EXISTS, FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_FOR_BACKUP_INTENT, FILE_OPEN_IF, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT};
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, RtlNtStatusToDosError, HANDLE, UNICODE_STRING};
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    AccessCheck, GetTokenInformation, TokenElevationType, TokenElevationTypeFull, TokenLinkedToken, DACL_SECURITY_INFORMATION, GENERIC_MAPPING,
    GROUP_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PRIVILEGE_SET, PSECURITY_DESCRIPTOR, TOKEN_ELEVATION_TYPE, TOKEN_LINKED_TOKEN, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::Storage::FileSystem::{
    FileDispositionInfo, GetFileInformationByHandle, GetFinalPathNameByHandleW, BY_HANDLE_FILE_INFORMATION, FILE_APPEND_DATA, FILE_ATTRIBUTE_REPARSE_POINT, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, SetFileInformationByHandle, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_ALL_ACCESS, FILE_DELETE_CHILD, FILE_GENERIC_EXECUTE, FILE_READ_DATA, WRITE_DAC, WRITE_OWNER, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_LIST_DIRECTORY, READ_CONTROL, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, SYNCHRONIZE,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

/// The plain `C:\…` (or `\\server\…`) form of a path Windows reports for a handle.
fn final_path(h: RawHandle) -> io::Result<PathBuf> {
    let mut buf = vec![0u16; 512];
    loop {
        let n = unsafe { GetFinalPathNameByHandleW(h as HANDLE, buf.as_mut_ptr(), buf.len() as u32, 0) } as usize;
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n < buf.len() {
            buf.truncate(n);
            break;
        }
        buf.resize(n + 1, 0);
    }
    let s = String::from_utf16_lossy(&buf);
    Ok(PathBuf::from(match s.strip_prefix(r"\\?\UNC\") {
        Some(r) => format!(r"\\{r}"),
        None => s.strip_prefix(r"\\?\").map(String::from).unwrap_or(s),
    }))
}

/// A folder opened for working in it. Without FILE_SHARE_DELETE: while it is open nobody can
/// move it, delete it or put a link in its place.
fn open_dir(p: &Path, pin: bool) -> io::Result<File> {
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE | if pin { 0 } else { FILE_SHARE_DELETE };
    std::fs::OpenOptions::new().access_mode(FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE).share_mode(share).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(p)
}

/// Where `p` really is (links followed), as a plain `C:\…` path.
pub fn real_path(p: &Path) -> io::Result<PathBuf> {
    let f = std::fs::OpenOptions::new().access_mode(FILE_READ_ATTRIBUTES).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(p)?;
    final_path(f.as_raw_handle())
}

/// `p` is `root` or inside it (both real paths; Windows paths ignore case).
fn is_under(p: &Path, root: &Path) -> bool {
    let (p, r) = (p.to_string_lossy().to_lowercase(), root.to_string_lossy().to_lowercase());
    let r = r.trim_end_matches('\\');
    p == r || p.strip_prefix(r).is_some_and(|rest| rest.starts_with('\\'))
}

fn plain_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['\\', '/', ':']) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("not a plain file name: {name:?}")));
    }
    Ok(())
}

/// `f`, unless it is a link (opened as itself, see [`Dir::open`]).
fn not_link(f: File, name: &str) -> io::Result<File> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(f.as_raw_handle() as HANDLE, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{name} is a link")));
    }
    Ok(f)
}

/// A token handle, closed when dropped.
struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// This process's own token (tests check with it when they run without admin rights).
#[cfg(test)]
fn own_token() -> io::Result<Token> {
    use windows_sys::Win32::Security::{DuplicateToken, SecurityIdentification, TOKEN_DUPLICATE};
    let mut t: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut t) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let own = Token(t);
    // as an everyday user even when the tests run as admin (CI runners do): Administrators only denies
    use windows_sys::Win32::Security::{CreateRestrictedToken, CreateWellKnownSid, WinBuiltinAdministratorsSid, SID_AND_ATTRIBUTES};
    let mut sid = [0u8; 68];
    let mut len = sid.len() as u32;
    if unsafe { CreateWellKnownSid(WinBuiltinAdministratorsSid, std::ptr::null_mut(), sid.as_mut_ptr() as _, &mut len) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let deny = SID_AND_ATTRIBUTES { Sid: sid.as_mut_ptr() as _, Attributes: 0 };
    let mut user: HANDLE = std::ptr::null_mut();
    if unsafe { CreateRestrictedToken(own.0, 0, 1, &deny, 0, std::ptr::null(), 0, std::ptr::null(), &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let user = Token(user);
    // AccessCheck wants an impersonation token
    let mut dup: HANDLE = std::ptr::null_mut();
    if unsafe { DuplicateToken(user.0, SecurityIdentification, &mut dup) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Token(dup))
}

/// The signed-in user's own token while Deck runs with admin rights: the limited one Windows
/// keeps for the same sign-in. None when Deck runs as the user (it has no rights of its own to lend).
fn limited_token() -> io::Result<Option<Token>> {
    let mut t: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut t) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let own = Token(t);
    let mut kind: TOKEN_ELEVATION_TYPE = 0;
    let mut n = 0u32;
    if unsafe { GetTokenInformation(own.0, TokenElevationType, &mut kind as *mut _ as *mut _, std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32, &mut n) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if kind != TokenElevationTypeFull {
        return Ok(None);
    }
    let mut linked = TOKEN_LINKED_TOKEN { LinkedToken: std::ptr::null_mut() };
    if unsafe { GetTokenInformation(own.0, TokenLinkedToken, &mut linked as *mut _ as *mut _, std::mem::size_of::<TOKEN_LINKED_TOKEN>() as u32, &mut n) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Some(Token(linked.LinkedToken)))
}

/// Whether `token` is allowed `access` to the open file or folder `f` (opened with READ_CONTROL),
/// by its permissions and integrity label, as Windows itself would decide.
fn token_may(token: &Token, f: &File, access: u32) -> io::Result<bool> {
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let what = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
    let e = unsafe { GetSecurityInfo(f.as_raw_handle() as HANDLE, SE_FILE_OBJECT, what, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut sd) };
    if e != 0 {
        return Err(io::Error::from_raw_os_error(e as i32));
    }
    let map = GENERIC_MAPPING { GenericRead: FILE_GENERIC_READ, GenericWrite: FILE_GENERIC_WRITE, GenericExecute: FILE_GENERIC_EXECUTE, GenericAll: FILE_ALL_ACCESS };
    // room for the privileges AccessCheck may report (a u64 buffer keeps it aligned)
    let mut privs = [0u64; 32];
    let mut plen = std::mem::size_of_val(&privs) as u32;
    let (mut granted, mut ok) = (0u32, 0);
    let r = unsafe { AccessCheck(sd, token.0, access, &map, privs.as_mut_ptr() as *mut PRIVILEGE_SET, &mut plen, &mut granted, &mut ok) };
    unsafe { LocalFree(sd as _) };
    if r == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ok != 0)
}

/// The folder `path`, for writing in on the signed-in user's behalf, made if missing. Links on the
/// way are followed (the user's own links, such as skills kept in another folder, keep working),
/// but the folder it really is must be one the user could add files to themselves: then Deck's
/// admin rights add nothing, wherever a link points. Missing folders are made one by one below
/// the deepest one that exists, once that one passes the same check.
pub fn open_for_user(path: &Path) -> io::Result<Dir> {
    open_for(limited_token()?.as_ref(), path)
}

fn open_for(token: Option<&Token>, path: &Path) -> io::Result<Dir> {
    let mut have = path;
    let mut missing: Vec<String> = Vec::new();
    while !have.exists() {
        let name = have.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{} has no folder to make it in", path.display())))?;
        missing.push(name.to_string_lossy().to_string());
        have = have.parent().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))?;
    }
    let h = open_dir(have, true)?;
    let mut d = Dir { real: final_path(h.as_raw_handle())?, h };
    // on a network share admin rights don't count (the share sees the same user either way)
    let token = token.filter(|_| !d.on_network());
    let refuse = |d: &Dir| {
        crate::log::warn(format!("refused: {} leads to {}, which isn't the signed-in user's to write in", path.display(), d.real.display()));
        Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{} isn't yours to write in", d.real.display())))
    };
    if let Some(t) = token {
        let need = if missing.is_empty() { FILE_ADD_FILE } else { FILE_ADD_SUBDIRECTORY };
        if !token_may(t, &d.h, need)? {
            return refuse(&d);
        }
    }
    for name in missing.iter().rev() {
        d = d.child(name, true)?;
    }
    if let Some(t) = token {
        if !missing.is_empty() && !token_may(t, &d.h, FILE_ADD_FILE)? {
            return refuse(&d);
        }
    }
    Ok(d)
}

fn refused(what: String) -> io::Error {
    crate::log::warn(format!("refused: {what}"));
    io::Error::new(io::ErrorKind::PermissionDenied, what)
}

/// Opens the file `p` for reading on the signed-in user's behalf (to send it somewhere): links are
/// followed, but the file it really is must be one the user could read themselves, so Deck's admin
/// rights never hand out a file the user couldn't open.
pub fn open_read_for_user(p: &Path) -> io::Result<File> {
    open_read_for(limited_token()?.as_ref(), p)
}

fn open_read_for(token: Option<&Token>, p: &Path) -> io::Result<File> {
    let f = std::fs::OpenOptions::new().access_mode(FILE_GENERIC_READ | READ_CONTROL).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).open(p)?;
    if let Some(t) = token {
        let real = final_path(f.as_raw_handle())?;
        if !real.to_string_lossy().starts_with(r"\\") && !token_may(t, &f, FILE_READ_DATA)? {
            return Err(refused(format!("{} isn't the signed-in user's to read", real.display())));
        }
    }
    Ok(f)
}

/// Fails unless the signed-in user could read everything in the folder `p` themselves, and it
/// holds no links (a zip of it could follow one somewhere else).
pub fn user_may_read_tree(p: &Path) -> io::Result<()> {
    tree_for(limited_token()?.as_ref(), p)
}

fn tree_for(token: Option<&Token>, p: &Path) -> io::Result<()> {
    let d = open_dir(p, false)?;
    if let Some(t) = token {
        if !token_may(t, &d, FILE_LIST_DIRECTORY)? {
            return Err(refused(format!("{} isn't the signed-in user's to read", p.display())));
        }
    }
    for e in std::fs::read_dir(p)? {
        let e = e?;
        let ft = e.file_type()?;
        if ft.is_symlink() || e.metadata().map(|m| std::os::windows::fs::MetadataExt::file_attributes(&m) & FILE_ATTRIBUTE_REPARSE_POINT != 0).unwrap_or(false) {
            return Err(refused(format!("{} is a link", e.path().display())));
        }
        if ft.is_dir() {
            tree_for(token, &e.path())?;
        } else {
            drop(open_read_for(token, &e.path())?);
        }
    }
    Ok(())
}

/// Whether the signed-in user (without admin rights) could change the program `exe`: write,
/// replace or re-permission it, add files next to it (a DLL it would load), or rename or replace
/// any folder above it. A program Deck has Windows start with admin rights must not be. False when
/// Deck runs without admin rights (there is no stronger account to protect).
pub fn user_could_change(exe: &Path) -> io::Result<bool> {
    could_change(limited_token()?.as_ref(), exe)
}

fn could_change(token: Option<&Token>, exe: &Path) -> io::Result<bool> {
    let Some(t) = token else { return Ok(false) };
    // AccessCheck answers for all the rights asked at once: each one on its own
    let any = |f: &File, rights: &[u32]| -> io::Result<bool> {
        for r in rights {
            if token_may(t, f, *r)? {
                return Ok(true);
            }
        }
        Ok(false)
    };
    let f = std::fs::OpenOptions::new().access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).open(exe)?;
    if any(&f, &[FILE_WRITE_DATA, FILE_APPEND_DATA, DELETE, WRITE_DAC, WRITE_OWNER])? {
        return Ok(true);
    }
    let mut first = true;
    let mut dir = exe.parent();
    while let Some(d) = dir.filter(|d| !d.as_os_str().is_empty()) {
        let h = open_dir(d, false)?;
        let rights: &[u32] = if first { &[FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_DELETE_CHILD, DELETE, WRITE_DAC, WRITE_OWNER] } else { &[FILE_DELETE_CHILD, DELETE, WRITE_DAC, WRITE_OWNER] };
        if any(&h, rights)? {
            return Ok(true);
        }
        first = false;
        dir = d.parent();
    }
    Ok(false)
}

/// Fails unless the signed-in user could delete `p` themselves (links on the way followed: what
/// counts is the file it really is). Always fine when Deck runs without admin rights.
pub fn user_may_delete(p: &Path) -> io::Result<()> {
    may_delete_for(limited_token()?.as_ref(), p)
}

fn may_delete_for(token: Option<&Token>, p: &Path) -> io::Result<()> {
    let Some(t) = token else { return Ok(()) };
    let (dir, name) = split(p)?;
    let h = open_dir(dir, false)?;
    let d = Dir { real: final_path(h.as_raw_handle())?, h };
    if d.on_network() {
        return Ok(());
    }
    // the name itself may be a link: then its target counts, so look where it really is
    match d.open(&name, READ_CONTROL | FILE_READ_ATTRIBUTES, FILE_OPEN, 0).and_then(|f| not_link(f, &name)) {
        Ok(_) => d.user_may_replace(t, &name),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            let real = real_path(p)?;
            let (rd, rn) = split(&real)?;
            let h = open_dir(rd, false)?;
            Dir { real: final_path(h.as_raw_handle())?, h }.user_may_replace(t, &rn)
        }
        Err(e) => Err(e),
    }
}

fn split(p: &Path) -> io::Result<(&Path, String)> {
    match (p.parent(), p.file_name()) {
        (Some(d), Some(n)) => Ok((d, n.to_string_lossy().to_string())),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a file in a folder", p.display()))),
    }
}

/// Moves `from` (a file or a folder; a link as itself) to `to` on the signed-in user's behalf,
/// only when they could take `from` away and put `to` there themselves (see [`open_for_user`]).
/// Both must be on the same drive.
pub fn move_for_user(from: &Path, to: &Path) -> io::Result<()> {
    move_for(limited_token()?.as_ref(), from, to)
}

fn move_for(token: Option<&Token>, from: &Path, to: &Path) -> io::Result<()> {
    let ((fd, name), (td, new)) = (split(from)?, split(to)?);
    let src = open_for(token, fd)?;
    let dst = open_for(token, td)?;
    if let Some(t) = token {
        if !src.on_network() {
            src.user_may_replace(t, &name)?;
        }
        if !dst.on_network() {
            dst.user_may_replace(t, &new)?;
        }
    }
    src.move_to(&name, &dst, &new, false)
}

/// Replaces the file `path` with `data` on the signed-in user's behalf (see [`open_for_user`]),
/// so that a crash leaves the old file or the new one, never a cut-off mix: the data goes to a
/// temp file of its own, reaches the disk, then takes the name.
pub fn write_for_user(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::io::Write;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let (dir, name) = split(path)?;
    let d = open_for_user(dir)?;
    d.check_replace(&name)?;
    let tmp = format!("{name}.{}-{}.tmp", std::process::id(), SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let res = (|| {
        let mut f = d.create(&tmp, true)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        // another program reading the file at that moment can make the rename fail briefly
        let mut last = Ok(());
        for _ in 0..5 {
            last = d.rename(&tmp, &name, true);
            if last.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        last
    })();
    if res.is_err() {
        let _ = d.remove(&tmp);
    }
    res
}

/// A folder pinned by its handle, known to be inside its root.
pub struct Dir {
    h: File,
    real: PathBuf,
}

impl Dir {
    /// The folder `path`, opened and held, if it really is `root_real` or inside it.
    pub fn open_under(root_real: &Path, path: &Path) -> io::Result<Dir> {
        let h = open_dir(path, true)?;
        let real = final_path(h.as_raw_handle())?;
        if !is_under(&real, root_real) {
            crate::log::warn(format!("refused: {} leads to {}, outside {}", path.display(), real.display(), root_real.display()));
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{} leads outside {}", path.display(), root_real.display())));
        }
        Ok(Dir { h, real })
    }

    /// Where this folder really is.
    pub fn real(&self) -> &Path {
        &self.real
    }

    /// `name` in this folder, opened relative to its handle (never through a path) and never
    /// through a link: a link named `name` is opened as the link itself.
    fn open(&self, name: &str, access: u32, disposition: u32, options: u32) -> io::Result<File> {
        self.open_share(name, access, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, disposition, options)
    }

    fn open_share(&self, name: &str, access: u32, share: u32, disposition: u32, options: u32) -> io::Result<File> {
        plain_name(name)?;
        let w: Vec<u16> = OsStr::new(name).encode_wide().collect();
        let bytes = (w.len() * 2) as u16;
        let us = UNICODE_STRING { Length: bytes, MaximumLength: bytes, Buffer: w.as_ptr() as *mut u16 };
        const OBJ_CASE_INSENSITIVE: u32 = 0x40;
        let oa = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: self.h.as_raw_handle() as HANDLE,
            ObjectName: &us,
            Attributes: OBJ_CASE_INSENSITIVE,
            SecurityDescriptor: std::ptr::null(),
            SecurityQualityOfService: std::ptr::null(),
        };
        let mut h: HANDLE = std::ptr::null_mut();
        let mut iosb: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        let st = unsafe {
            NtCreateFile(
                &mut h,
                access | SYNCHRONIZE,
                &oa,
                &mut iosb,
                std::ptr::null(),
                FILE_ATTRIBUTE_NORMAL,
                share,
                disposition,
                options | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
                std::ptr::null(),
                0,
            )
        };
        if st < 0 {
            return Err(io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(st) } as i32));
        }
        Ok(unsafe { File::from_raw_handle(h as RawHandle) })
    }

    /// Creates (or with `replace`, replaces) `name` in this folder for writing. A link in its place
    /// is replaced, never written through.
    pub fn create(&self, name: &str, replace: bool) -> io::Result<File> {
        if replace {
            match self.remove(name) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        self.open(name, FILE_GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES, FILE_CREATE, FILE_NON_DIRECTORY_FILE)
    }

    /// The folder `name` inside this one (made when `create`), held like this one. A link in its
    /// place is refused.
    pub fn child(&self, name: &str, create: bool) -> io::Result<Dir> {
        let access = FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL;
        let h = self.open_share(name, access, FILE_SHARE_READ | FILE_SHARE_WRITE, if create { FILE_OPEN_IF } else { FILE_OPEN }, FILE_DIRECTORY_FILE)?;
        Ok(Dir { h: not_link(h, name)?, real: self.real.join(name) })
    }

    /// The folder holding `rel` (a path below this folder, `/` or `\` between names) and the last
    /// name in it. Folders on the way are reached one by one through [`Dir::child`] (made when
    /// `create`), so none of them can be a link.
    pub fn walk(&self, rel: &str, create: bool) -> io::Result<(Dir, String)> {
        let mut parts: Vec<&str> = rel.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
        let leaf = parts.pop().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty path"))?;
        plain_name(leaf)?;
        let mut d = Dir { h: self.h.try_clone()?, real: self.real.clone() };
        for p in parts {
            d = d.child(p, create)?;
        }
        Ok((d, leaf.to_string()))
    }

    /// Opens the file `name` in this folder for reading and writing, as it is (a link in its place
    /// is refused).
    pub fn edit(&self, name: &str) -> io::Result<File> {
        not_link(self.open(name, FILE_GENERIC_READ | FILE_GENERIC_WRITE, FILE_OPEN, FILE_NON_DIRECTORY_FILE)?, name)
    }

    /// Fails unless the signed-in user (`token`, see [`limited_token`]) could delete or replace
    /// `name` in this folder themselves. Nothing there yet is fine.
    fn user_may_replace(&self, token: &Token, name: &str) -> io::Result<()> {
        let f = match self.open(name, READ_CONTROL | FILE_READ_ATTRIBUTES, FILE_OPEN, 0) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if token_may(token, &f, DELETE)? || token_may(token, &self.h, FILE_DELETE_CHILD)? {
            return Ok(());
        }
        crate::log::warn(format!("refused: {} in {} isn't the signed-in user's to replace", name, self.real.display()));
        Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{name} isn't yours to replace")))
    }

    /// Fails unless the signed-in user could replace `name` here themselves (see
    /// [`open_for_user`]); always fine when Deck runs without admin rights.
    pub fn check_replace(&self, name: &str) -> io::Result<()> {
        match limited_token()? {
            Some(t) if !self.on_network() => self.user_may_replace(&t, name),
            _ => Ok(()),
        }
    }

    fn on_network(&self) -> bool {
        self.real.to_string_lossy().starts_with(r"\\")
    }

    /// Moves `name` from this folder to `new` in the folder `to` (same drive), by handle on both
    /// sides. A link is moved as itself.
    pub fn move_to(&self, name: &str, to: &Dir, new: &str, replace: bool) -> io::Result<()> {
        to.rename_from(self, name, new, replace)
    }

    /// Opens `name` in this folder for adding to its end (made if it isn't there). A link in its
    /// place is refused, never written through.
    pub fn append(&self, name: &str) -> io::Result<File> {
        // append-only access: with plain write access too, writes would go where the file pointer is
        not_link(self.open(name, FILE_APPEND_DATA | FILE_WRITE_ATTRIBUTES | FILE_READ_ATTRIBUTES, FILE_OPEN_IF, FILE_NON_DIRECTORY_FILE)?, name)
    }

    /// The size of the file `name` in this folder (a link in its place is refused).
    pub fn len(&self, name: &str) -> io::Result<u64> {
        Ok(not_link(self.open(name, FILE_READ_ATTRIBUTES, FILE_OPEN, FILE_NON_DIRECTORY_FILE)?, name)?.metadata()?.len())
    }

    /// Cuts the file `name` in this folder back to `len` bytes (a link in its place is refused).
    pub fn truncate(&self, name: &str, len: u64) -> io::Result<()> {
        not_link(self.open(name, FILE_WRITE_DATA | FILE_READ_ATTRIBUTES, FILE_OPEN, FILE_NON_DIRECTORY_FILE)?, name)?.set_len(len)
    }

    /// Deletes `name` (a file, an empty folder, or a link itself, never what it points to).
    pub fn remove(&self, name: &str) -> io::Result<()> {
        let f = self.open(name, DELETE | FILE_READ_ATTRIBUTES, FILE_OPEN, FILE_OPEN_FOR_BACKUP_INTENT)?;
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        let ok = unsafe { SetFileInformationByHandle(f.as_raw_handle() as HANDLE, FileDispositionInfo, &info as *const _ as *const _, std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Renames `from` to `to`, both in this folder (the new name is given relative to the held
    /// folder, so it can't land anywhere else).
    pub fn rename(&self, from: &str, to: &str, replace: bool) -> io::Result<()> {
        self.rename_from(self, from, to, replace)
    }

    /// `from` in the folder `src` becomes `to` in this one.
    fn rename_from(&self, src: &Dir, from: &str, to: &str, replace: bool) -> io::Result<()> {
        plain_name(to)?;
        let f = src.open(from, DELETE | FILE_READ_ATTRIBUTES, FILE_OPEN, FILE_OPEN_FOR_BACKUP_INTENT)?;
        let name: Vec<u16> = OsStr::new(to).encode_wide().collect();
        let size = std::mem::size_of::<FILE_RENAME_INFORMATION>() + name.len() * 2;
        // a u64 buffer keeps the structure aligned
        let mut buf = vec![0u64; size.div_ceil(8)];
        let info = buf.as_mut_ptr() as *mut FILE_RENAME_INFORMATION;
        // the native call: kernel32's wrapper refuses a name relative to a folder handle
        let set = |class, posix: bool| unsafe {
            if posix {
                // like std::fs::rename: replaces a file another program has open (and reading)
                (*info).Anonymous.Flags = FILE_RENAME_POSIX_SEMANTICS | if replace { FILE_RENAME_REPLACE_IF_EXISTS } else { 0 };
            } else {
                (*info).Anonymous.Flags = 0;
                (*info).Anonymous.ReplaceIfExists = replace;
            }
            (*info).RootDirectory = self.h.as_raw_handle() as HANDLE;
            (*info).FileNameLength = (name.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(name.as_ptr(), std::ptr::addr_of_mut!((*info).FileName) as *mut u16, name.len());
            let mut iosb: IO_STATUS_BLOCK = std::mem::zeroed();
            NtSetInformationFile(f.as_raw_handle() as HANDLE, &mut iosb, info as *const _, size as u32, class)
        };
        let mut st = set(FileRenameInformationEx, true);
        // drives without it (FAT, some network shares): the classic rename
        const INVALID_INFO_CLASS: i32 = 0xC000_0003_u32 as i32;
        const INVALID_PARAMETER: i32 = 0xC000_000D_u32 as i32;
        const NOT_SUPPORTED: i32 = 0xC000_00BB_u32 as i32;
        if [INVALID_INFO_CLASS, INVALID_PARAMETER, NOT_SUPPORTED].contains(&st) {
            st = set(FileRenameInformation, false);
        }
        if st < 0 {
            return Err(io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(st) } as i32));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, Write};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deck-fsguard-{name}-{}", crate::util::now_ms()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A folder link (junction) like one another program could plant: no admin rights needed.
    fn junction(link: &Path, target: &Path) {
        use std::os::windows::process::CommandExt;
        let ok = std::process::Command::new("cmd.exe").args(["/c", "mklink", "/J"]).arg(link).arg(target).creation_flags(crate::util::CREATE_NO_WINDOW).output().unwrap();
        assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    }

    #[test]
    fn ordinary_work_inside_the_root() {
        let root = tmp("ok");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let real = real_path(&root).unwrap();
        let d = Dir::open_under(&real, &root.join("sub")).unwrap();
        d.create("a.part", false).unwrap().write_all(b"hello").unwrap();
        assert!(d.create("a.part", false).is_err(), "never overwrites unless asked");
        {
            let mut a = d.append("a.part").unwrap();
            a.write_all(b" world!!").unwrap();
        }
        // a cut-off piece is trimmed off again
        d.truncate("a.part", 11).unwrap();
        assert_eq!(d.len("a.part").unwrap(), 11);
        assert_eq!(d.len("missing.part").unwrap_err().kind(), io::ErrorKind::NotFound);
        d.rename("a.part", "a.txt", false).unwrap();
        assert_eq!(std::fs::read(root.join("sub").join("a.txt")).unwrap(), b"hello world");
        d.append("new.part").unwrap().write_all(b"x").unwrap();
        assert_eq!(std::fs::read(root.join("sub").join("new.part")).unwrap(), b"x", "append makes a missing file");
        d.remove("new.part").unwrap();
        d.remove("a.txt").unwrap();
        assert!(!root.join("sub").join("a.txt").exists());
        let top = Dir::open_under(&real, &root).unwrap();
        // a held folder can't be deleted (or moved, or swapped): that is the point of holding it
        assert!(top.remove("sub").is_err());
        drop(d);
        top.remove("sub").unwrap();
        assert!(!root.join("sub").exists());
        for bad in ["..", "x\\..\\y", "a/b", "", "C:x"] {
            assert!(top.create(bad, true).is_err(), "{bad:?} is not a plain name");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A folder inside the root swapped for a link to elsewhere: nothing outside is touched.
    #[test]
    fn a_link_out_of_the_root_is_refused() {
        let (root, outside) = (tmp("root"), tmp("outside"));
        std::fs::write(outside.join("victim.dll"), b"keep me").unwrap();
        junction(&root.join("cache"), &outside);
        let real = real_path(&root).unwrap();
        assert!(Dir::open_under(&real, &root.join("cache")).is_err(), "the link leads outside the root");
        let top = Dir::open_under(&real, &root).unwrap();
        assert!(top.append("cache").is_err(), "a link in a file's place is never written through");
        assert!(top.len("cache").is_err(), "nor measured as if it were the file");
        drop(top);
        // the link itself can still go (it's inside the root); what it points to stays
        Dir::open_under(&real, &root).unwrap().remove("cache").unwrap();
        assert!(!root.join("cache").exists());
        assert_eq!(std::fs::read(outside.join("victim.dll")).unwrap(), b"keep me");
        let _ = (std::fs::remove_dir_all(&root), std::fs::remove_dir_all(&outside));
    }

    /// Received files go into subfolders: each is reached through the one above it, never through
    /// a link, so a subfolder swapped for a link to elsewhere stops the work instead of following it.
    #[test]
    fn nested_folders_are_walked_without_links() {
        let (root, outside) = (tmp("walk"), tmp("walk-out"));
        let real = real_path(&root).unwrap();
        let d = Dir::open_under(&real, &root).unwrap();
        let (p, leaf) = d.walk("a/b/c.txt", true).unwrap();
        assert_eq!(leaf, "c.txt");
        assert!(p.real().ends_with("a\\b"), "{}", p.real().display());
        p.create(&leaf, true).unwrap().write_all(b"abcdef").unwrap();
        assert_eq!(std::fs::read(root.join("a").join("b").join("c.txt")).unwrap(), b"abcdef");
        {
            // a part file continued: opened for reading and writing where it is
            let mut f = p.edit(&leaf).unwrap();
            f.set_len(3).unwrap();
            f.seek(std::io::SeekFrom::End(0)).unwrap();
            f.write_all(b"XYZ").unwrap();
        }
        assert_eq!(std::fs::read(root.join("a").join("b").join("c.txt")).unwrap(), b"abcXYZ");
        assert!(d.walk("a/missing/c.txt", false).is_err(), "nothing is made unless asked");
        assert!(!root.join("a").join("missing").exists());
        let (top, leaf) = d.walk("top.txt", false).unwrap();
        assert_eq!((top.real(), leaf.as_str()), (d.real(), "top.txt"));
        drop((p, top));

        junction(&root.join("x"), &outside);
        assert!(d.walk("x/y.txt", true).is_err(), "a folder that is a link is not gone through");
        assert!(d.walk("x/deeper/y.txt", true).is_err());
        assert!(d.edit("x").is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0, "nothing was made outside");
        drop(d);
        let _ = std::fs::remove_dir(root.join("x"));
        let _ = (std::fs::remove_dir_all(&root), std::fs::remove_dir_all(&outside));
    }

    /// A token to check with in tests, whether or not the tests run with admin rights: the
    /// signed-in user's own, like the real check uses.
    fn test_token() -> Token {
        limited_token().unwrap().unwrap_or_else(|| own_token().unwrap())
    }

    #[test]
    fn the_user_can_add_files_to_temp_but_not_to_windows() {
        let t = test_token();
        let sys = open_dir(&crate::util::system_dir(), false).unwrap();
        assert!(!token_may(&t, &sys, FILE_ADD_FILE).unwrap(), "System32 is not the user's to write in");
        let root = tmp("may");
        let mine = open_dir(&root, false).unwrap();
        assert!(token_may(&t, &mine, FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY).unwrap());
        drop(mine);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The user's own links keep working (skills kept in another folder of theirs); a link to a
    /// folder the user couldn't write in themselves is refused, and nothing is made there.
    #[test]
    fn writing_for_the_user_goes_only_where_they_could_write() {
        let t = test_token();
        let (root, theirs) = (tmp("foruser"), tmp("foruser-elsewhere"));
        let d = open_for(Some(&t), &root.join("new").join("deeper")).unwrap();
        d.create("a.txt", false).unwrap();
        assert!(root.join("new").join("deeper").join("a.txt").exists(), "missing folders are made");
        drop(d);

        junction(&root.join("skills"), &theirs);
        let d = open_for(Some(&t), &root.join("skills")).unwrap();
        assert!(d.real().ends_with(theirs.file_name().unwrap()), "{}", d.real().display());
        d.user_may_replace(&t, "missing.txt").unwrap();
        drop(d);

        junction(&root.join("sys"), &crate::util::system_dir());
        assert!(open_for(Some(&t), &root.join("sys")).is_err(), "a link into Windows is refused");
        assert!(open_for(Some(&t), &root.join("sys").join("deck-test-new")).is_err());
        assert!(!crate::util::system_dir().join("deck-test-new").exists(), "nothing was made in Windows");
        let _ = std::fs::remove_dir(root.join("sys"));
        let _ = std::fs::remove_dir(root.join("skills"));
        let _ = (std::fs::remove_dir_all(&root), std::fs::remove_dir_all(&theirs));
    }

    /// Moving out of (or into) a folder the user can't change themselves is refused before
    /// anything is touched.
    #[test]
    fn moving_for_the_user_checks_both_sides() {
        let t = test_token();
        let (root, other) = (tmp("mv-user"), tmp("mv-user-to"));
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        move_for(Some(&t), &root.join("a.txt"), &other.join("sub").join("a.txt")).unwrap();
        assert_eq!(std::fs::read(other.join("sub").join("a.txt")).unwrap(), b"a");
        junction(&root.join("sys"), &crate::util::system_dir());
        // a name that doesn't exist there: even a broken check couldn't move anything real
        assert!(move_for(Some(&t), &root.join("sys").join("deck-test-none.txt"), &other.join("x.txt")).is_err());
        assert!(move_for(Some(&t), &other.join("sub").join("a.txt"), &root.join("sys").join("deck-test-new.txt")).is_err());
        assert!(other.join("sub").join("a.txt").exists(), "refused before moving");
        assert!(!crate::util::system_dir().join("deck-test-new.txt").exists());
        let _ = std::fs::remove_dir(root.join("sys"));
        let _ = (std::fs::remove_dir_all(&root), std::fs::remove_dir_all(&other));
    }

    #[test]
    fn writing_a_file_for_the_user() {
        let root = tmp("wfu");
        let f = root.join("deeper").join("x.json");
        write_for_user(&f, b"one").unwrap();
        write_for_user(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        let names: Vec<String> = std::fs::read_dir(root.join("deeper")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["x.json".to_string()], "no temp file left over");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reading_for_the_user() {
        let t = test_token();
        let root = tmp("read");
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut open_read_for(Some(&t), &root.join("a.txt")).unwrap(), &mut s).unwrap();
        assert_eq!(s, "a");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("b.txt"), b"b").unwrap();
        tree_for(Some(&t), &root).unwrap();
        junction(&root.join("sub").join("sys"), &crate::util::system_dir());
        assert!(tree_for(Some(&t), &root).is_err(), "a folder with a link in it isn't zipped (the zip could follow it)");
        let _ = std::fs::remove_dir(root.join("sub").join("sys"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Windows' own programs are safe to start with admin rights; a program in a folder the user
    /// owns is not (any program of theirs could replace it, or put a DLL next to it).
    #[test]
    fn programs_the_user_could_change() {
        let t = test_token();
        assert!(!could_change(Some(&t), &crate::util::system_dir().join("cmd.exe")).unwrap());
        let root = tmp("exe");
        std::fs::write(root.join("deck.exe"), b"MZ").unwrap();
        assert!(could_change(Some(&t), &root.join("deck.exe")).unwrap());
        assert!(!could_change(None, &root.join("deck.exe")).unwrap(), "without admin rights there is nothing to protect");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Clean-up and the Recycle Bin delete only what the user could delete themselves.
    #[test]
    fn deleting_for_the_user() {
        let t = test_token();
        let root = tmp("del");
        std::fs::write(root.join("a.tmp"), b"a").unwrap();
        may_delete_for(Some(&t), &root.join("a.tmp")).unwrap();
        assert!(may_delete_for(Some(&t), &crate::util::system_dir().join("cmd.exe")).is_err());
        junction(&root.join("sys"), &crate::util::system_dir());
        assert!(may_delete_for(Some(&t), &root.join("sys").join("cmd.exe")).is_err(), "nor through a link");
        let _ = std::fs::remove_dir(root.join("sys"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file moved to another held folder (Claude Sync's trash), by handle on both sides.
    #[test]
    fn moving_between_held_folders() {
        let (a, b) = (tmp("move-a"), tmp("move-b"));
        std::fs::write(a.join("x.txt"), b"x").unwrap();
        let (da, db) = (Dir::open_under(&real_path(&a).unwrap(), &a).unwrap(), Dir::open_under(&real_path(&b).unwrap(), &b).unwrap());
        da.move_to("x.txt", &db, "y.txt", false).unwrap();
        assert!(!a.join("x.txt").exists());
        assert_eq!(std::fs::read(b.join("y.txt")).unwrap(), b"x");
        drop((da, db));
        let _ = (std::fs::remove_dir_all(&a), std::fs::remove_dir_all(&b));
    }

    /// The root itself may be a link (Downloads moved to another drive): that's where work goes.
    #[test]
    fn a_root_that_is_a_link_is_followed() {
        let (base, target) = (tmp("base"), tmp("target"));
        junction(&base.join("Downloads"), &target);
        let real = real_path(&base.join("Downloads")).unwrap();
        assert_eq!(real.file_name(), target.file_name());
        Dir::open_under(&real, &base.join("Downloads")).unwrap().create("x.txt", true).unwrap();
        assert!(target.join("x.txt").exists());
        let _ = std::fs::remove_dir_all(&target);
        let _ = std::fs::remove_dir(base.join("Downloads"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
