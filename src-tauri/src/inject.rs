//! Inject `mosickwhile_bridge.dll` into a target process (exclusive fullscreen path).

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, WaitForSingleObject, PROCESS_CREATE_THREAD,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

pub fn bridge_dll_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no exe dir")?;
    let candidates = [
        dir.join("mosickwhile_bridge.dll"),
        dir.join("resources").join("mosickwhile_bridge.dll"),
        dir.join("bridge").join("mosickwhile_bridge.dll"),
        // Dev / workspace
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mosickwhile_bridge.dll"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("target")
            .join("release")
            .join("mosickwhile_bridge.dll"),
    ];
    for c in candidates {
        if c.is_file() {
            return Ok(c.canonicalize().unwrap_or(c));
        }
    }
    Err(
        "mosickwhile_bridge.dll not found next to the executable. Rebuild with npm run build:bridge."
            .into(),
    )
}

pub fn inject_pid(pid: u32) -> Result<PathBuf, String> {
    let dll = bridge_dll_path()?;
    let dll_wide: Vec<u16> = OsStr::new(&dll)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let bytes = (dll_wide.len() * 2) as usize;

    unsafe {
        let process = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            false,
            pid,
        )
        .map_err(|e| format!("OpenProcess({pid}): {e} — run as admin if needed"))?;

        let remote = VirtualAllocEx(
            process,
            None,
            bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote.is_null() {
            let _ = CloseHandle(process);
            return Err("VirtualAllocEx failed".into());
        }

        let mut written = 0usize;
        WriteProcessMemory(
            process,
            remote,
            dll_wide.as_ptr() as *const _,
            bytes,
            Some(&mut written),
        )
        .map_err(|e| {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            format!("WriteProcessMemory: {e}")
        })?;

        let k32_name: Vec<u16> = OsStr::new("kernel32.dll")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let k32 = GetModuleHandleW(PCWSTR(k32_name.as_ptr()))
            .map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let load_library = GetProcAddress(k32, windows::core::s!("LoadLibraryW"))
            .ok_or("GetProcAddress(LoadLibraryW) failed")?;

        let thread = CreateRemoteThread(
            process,
            None,
            0,
            Some(std::mem::transmute(load_library)),
            Some(remote),
            0,
            None,
        )
        .map_err(|e| {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            format!("CreateRemoteThread: {e}")
        })?;

        let _ = WaitForSingleObject(thread, 10_000);
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(process);
    }

    Ok(dll)
}

pub fn foreground_pid() -> Result<u32, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err("no foreground window".into());
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return Err("could not resolve foreground PID".into());
        }
        if pid == std::process::id() {
            return Err("foreground is MOSICKWHILE itself — focus the game first".into());
        }
        Ok(pid)
    }
}
