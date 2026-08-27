//! Windows 进程工具（存活检测、进程树终止）。

/// 判断 PID 对应的进程是否仍存活。
#[cfg(windows)]
pub fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    if pid == 0 {
        return false;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            // ERROR_INVALID_PARAMETER 表示进程不存在；其余错误（如无权限）视为存活
            GetLastError() != ERROR_INVALID_PARAMETER
        } else {
            CloseHandle(h);
            true
        }
    }
}

/// 以 taskkill 强制结束进程树（CREATE_NO_WINDOW，避免闪黑窗）。
pub fn kill_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    let mut cmd = std::process::Command::new("taskkill");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let _ = cmd.args(["/PID", &pid.to_string(), "/F", "/T"]).output();
}
