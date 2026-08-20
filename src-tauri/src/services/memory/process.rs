// RAII process handle for reading MTGA.exe memory.
//
// Wraps Windows Toolhelp32 for process discovery and VirtualQueryEx for
// region enumeration. The handle auto-closes on drop.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{
    VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_PRIVATE, PAGE_GUARD, PAGE_NOACCESS,
    PAGE_PROTECTION_FLAGS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};

use super::scanner::ENTRY_SIZE;

/// Minimum entries a region must hold to be worth scanning.
const MIN_REGION_ENTRIES: usize = 100;

/// A readable committed memory region.
pub struct MemRegion {
    pub base: usize,
    pub size: usize,
}

/// RAII wrapper around a Windows process handle.
///
/// SAFETY: The handle is opened with PROCESS_VM_READ | PROCESS_QUERY_INFORMATION.
/// ReadProcessMemory is thread-safe when called with a valid handle — the kernel
/// serializes access. The handle is never mutated after creation.
pub struct ProcessHandle {
    handle: HANDLE,
    pub pid: u32,
}

// SAFETY: See struct-level comment. Read-only handle, kernel-serialized.
unsafe impl Send for ProcessHandle {}
unsafe impl Sync for ProcessHandle {}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

impl ProcessHandle {
    /// Open a process by PID with read-only access.
    pub fn open(pid: u32) -> Result<Self, String> {
        let handle = unsafe {
            OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
        }
        .map_err(|e| format!("OpenProcess failed for PID {}: {}", pid, e))?;

        Ok(Self { handle, pid })
    }

    /// Enumerate all readable committed memory regions.
    pub fn enumerate_regions(&self) -> Result<Vec<MemRegion>, String> {
        let mut regions = Vec::new();
        let mut address: usize = 0;
        let max_address: usize = (1u64 << 47) as usize - 1; // User-space limit, 64-bit Windows
        let mbi_size = std::mem::size_of::<MEMORY_BASIC_INFORMATION>();

        while address < max_address {
            let mut mbi = MEMORY_BASIC_INFORMATION::default();
            let bytes_returned = unsafe {
                VirtualQueryEx(
                    self.handle,
                    Some(address as *const std::ffi::c_void),
                    &mut mbi,
                    mbi_size,
                )
            };

            if bytes_returned == 0 {
                break;
            }

            let region_size = mbi.RegionSize;
            if region_size == 0 {
                break;
            }

            if is_readable(&mbi) && region_size > ENTRY_SIZE * MIN_REGION_ENTRIES {
                regions.push(MemRegion {
                    base: mbi.BaseAddress as usize,
                    size: region_size,
                });
            }

            address = mbi.BaseAddress as usize + region_size;
        }

        Ok(regions)
    }

    /// Read process memory into the provided buffer. Returns bytes actually read.
    pub fn read_memory(&self, base: usize, buffer: &mut [u8]) -> Result<usize, String> {
        let mut bytes_read: usize = 0;
        unsafe {
            ReadProcessMemory(
                self.handle,
                base as *const std::ffi::c_void,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
                buffer.len(),
                Some(&mut bytes_read),
            )
        }
        .map_err(|e| format!("ReadProcessMemory at {:#x}: {}", base, e))?;

        Ok(bytes_read)
    }
}

/// Check if a memory region is committed, private, and readable.
///
/// .NET GC heap is always MEM_PRIVATE (VirtualAlloc). MEM_IMAGE (DLLs) and
/// MEM_MAPPED (shared memory, file views) cannot contain managed objects —
/// filtering them out eliminates a large portion of enumerated regions.
fn is_readable(mbi: &MEMORY_BASIC_INFORMATION) -> bool {
    if mbi.State != MEM_COMMIT {
        return false;
    }
    if mbi.Type != MEM_PRIVATE {
        return false;
    }
    let protect = mbi.Protect;
    if protect == PAGE_PROTECTION_FLAGS(0) {
        return false;
    }
    if protect.contains(PAGE_NOACCESS) || protect.contains(PAGE_GUARD) {
        return false;
    }
    true
}

/// Get the full path of a process's executable by PID.
pub fn process_exe_path(pid: u32) -> Result<std::path::PathBuf, String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(|e| format!("OpenProcess failed for PID {}: {}", pid, e))?;

    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    result.map_err(|e| format!("QueryFullProcessImageNameW failed for PID {}: {}", pid, e))?;

    Ok(std::path::PathBuf::from(String::from_utf16_lossy(
        &buf[..len as usize],
    )))
}

/// Find a running process by executable name (case-insensitive).
/// Returns the PID if found.
pub fn find_process(name: &str) -> Result<Option<u32>, String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|e| format!("CreateToolhelp32Snapshot failed: {}", e))?;

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let name_lower = name.to_lowercase();

    unsafe {
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let exe_name = String::from_utf16_lossy(
                    &entry.szExeFile[..entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len())],
                );

                if exe_name.to_lowercase() == name_lower {
                    let _ = CloseHandle(snapshot);
                    return Ok(Some(entry.th32ProcessID));
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }

    Ok(None)
}
