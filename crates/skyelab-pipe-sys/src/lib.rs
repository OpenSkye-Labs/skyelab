//! The Windows calls SkyeLab's MCP named pipe needs that have no safe binding: tokio takes a pipe's security
//! descriptor only as a raw `SECURITY_ATTRIBUTES` pointer, and reading the user's SID or a process's exit code is
//! plain Win32. Everything else (the transport, tokens, grants) stays in safe code in skyelab-agent. Windows only.
#![allow(unsafe_code)]

#[cfg(windows)]
pub use win::{create_private_pipe, pipe_server_pid, private_sddl, process_alive};

#[cfg(windows)]
mod win {
    use std::ffi::{OsStr, c_void};
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, HANDLE, LocalFree, STILL_ACTIVE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenProcessToken,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// A handle closed on drop.
    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: `self.0` is a handle this process opened and owns; it is closed once, here.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Memory Win32 allocated with `LocalAlloc` for us, freed on drop.
    struct Local(*mut c_void);

    impl Drop for Local {
        fn drop(&mut self) {
            // SAFETY: `self.0` came from a call documented to allocate with LocalAlloc; it is freed once, here.
            unsafe { LocalFree(self.0) };
        }
    }

    /// `D:P(A;;GA;;;<SID>)`: a protected DACL with one entry, full access for the user this process runs as. Nobody
    /// else (not Administrators, SYSTEM, or Everyone's default read on a pipe) can open it.
    pub fn private_sddl() -> io::Result<String> {
        Ok(format!("D:P(A;;GA;;;{})", current_user_sid()?))
    }

    fn current_user_sid() -> io::Result<String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess is a pseudo handle that needs no closing; `token` is a valid out pointer.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Handle(token);
        let mut len = 0u32;
        // SAFETY: a null buffer of length 0 only asks for the size needed, written to `len`.
        unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut len) };
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        // u64s, so the TOKEN_USER at the start (pointer-aligned) is aligned.
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: `buf` holds at least `len` writable bytes; `len` is a valid out pointer.
        let ok = unsafe {
            GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded, so `buf` starts with a TOKEN_USER whose SID points into `buf`, alive here.
        let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut text: *mut u16 = std::ptr::null_mut();
        // SAFETY: `sid` is a valid SID (above); `text` is a valid out pointer for a LocalAlloc'ed string.
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let text = Local(text.cast());
        let wide = text.0.cast::<u16>();
        // SAFETY: `wide` is the NUL-terminated UTF-16 string the call above wrote; we read up to its NUL.
        let len = (0..).take_while(|&i| unsafe { *wide.add(i) } != 0).count();
        // SAFETY: `len` u16s before the NUL are initialized, and `text` keeps them alive until after the copy.
        let units = unsafe { std::slice::from_raw_parts(wide, len) };
        Ok(String::from_utf16_lossy(units))
    }

    /// Creates a pipe instance only the current user can open ([`private_sddl`]). Call it inside a tokio runtime.
    pub fn create_private_pipe(
        options: &ServerOptions,
        name: &OsStr,
    ) -> io::Result<NamedPipeServer> {
        let sddl: Vec<u16> = private_sddl()?.encode_utf16().chain([0]).collect();
        let mut sd: *mut c_void = std::ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated UTF-16; `sd` is a valid out pointer for a LocalAlloc'ed descriptor.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let sd = Local(sd);
        let mut attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        // SAFETY: `attrs` is a valid SECURITY_ATTRIBUTES, and its descriptor (`sd`) outlives the call.
        unsafe { options.create_with_security_attributes_raw(name, (&raw mut attrs).cast()) }
    }

    /// The pid of the process serving the pipe `client` is connected to.
    pub fn pipe_server_pid(client: &impl AsRawHandle) -> io::Result<u32> {
        let mut pid = 0u32;
        // SAFETY: `client` keeps its pipe handle open for the call; `pid` is a valid out pointer.
        if unsafe { GetNamedPipeServerProcessId(client.as_raw_handle(), &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pid)
    }

    /// Whether `pid` still runs. Only "no such process" counts as gone: a process we may not query is kept.
    pub fn process_alive(pid: u32) -> bool {
        // SAFETY: a plain call; it returns a handle we own, or null.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return io::Error::last_os_error().raw_os_error()
                != Some(ERROR_INVALID_PARAMETER as i32);
        }
        let handle = Handle(handle);
        let mut code = 0u32;
        // SAFETY: `handle` is a valid process handle with query rights; `code` is a valid out pointer.
        let ok = unsafe { GetExitCodeProcess(handle.0, &mut code) } != 0;
        !ok || code == STILL_ACTIVE as u32
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn the_dacl_names_only_the_current_user() {
        let sddl = super::private_sddl().expect("sddl");
        let sid = sddl
            .strip_prefix("D:P(A;;GA;;;")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or_else(|| panic!("one protected allow entry: {sddl}"));
        assert!(
            sid.starts_with("S-1-5-"),
            "a real user SID, not an alias like WD or AN: {sid}"
        );
        assert!(!sid.contains('('), "exactly one entry: {sddl}");
    }

    #[test]
    fn this_process_is_alive_and_an_exited_one_is_not() {
        assert!(super::process_alive(std::process::id()));
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit"])
            .spawn()
            .expect("spawn");
        let pid = child.id();
        child.wait().expect("wait");
        drop(child); // closes our handle, so the process object can go
        assert!(!super::process_alive(pid));
    }
}
