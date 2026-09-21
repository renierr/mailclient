//! Native window decoration, where the toolkit does not handle it.
//!
//! Windows draws the caption bar itself, outside the Qt scene, and Qt 6 does
//! not ask DWM for a dark one — a fully dark app therefore gets a white title
//! bar on a dark desktop. `DWMWA_USE_IMMERSIVE_DARK_MODE` is the only knob for
//! it, so we set it on our own top-level windows.
//!
//! On Linux the compositor owns the decoration and follows the desktop
//! preference, so there is nothing to do; the function is a no-op.
//!
//! The console is the same shape of problem. `main.rs` marks the binary as a
//! Windows GUI app so double-clicking it does not flash up a terminal, which
//! also cuts the headless CLI (`--status` / `--sync-once`) off from the shell
//! that started it. [`attach_parent_console`] reconnects it when there is a
//! console to reconnect to.

/// Ask the window manager for dark (or light) decorations on every top-level
/// window of the calling thread — the GUI thread, where Qt creates them.
pub fn set_dark_decorations(dark: bool) {
    imp::set_dark_decorations(dark);
}

/// Reconnect stdout/stderr to the console that launched us, if any.
///
/// The binary is a Windows GUI subsystem app (see `main.rs`), so Windows
/// gives it no console and `println!` goes nowhere -- which would silence
/// `--status`, `--sync-once` and every log line when the app is run from a
/// terminal. Attaching the *parent's* console fixes that without ever
/// creating a window of its own: there is nothing to attach to when the user
/// double-clicks the exe, and the call simply fails.
///
/// Redirection (`mailapp --status --json > out.json`) and shells that hand
/// the child real pipes (MSYS2, Git Bash) already supply valid handles, so
/// those are left exactly as they are. No-op off Windows, where the
/// subsystem concept does not exist.
pub fn attach_parent_console() {
    imp::attach_parent_console();
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    type Hwnd = *mut c_void;

    // Documented since Windows 10 20H1; build 18985 and earlier used 19 for
    // the same thing, hence the fallback.
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_USE_IMMERSIVE_DARK_MODE_OLD: u32 = 19;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn EnumThreadWindows(
            thread_id: u32,
            callback: unsafe extern "system" fn(Hwnd, isize) -> i32,
            param: isize,
        ) -> i32;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: Hwnd,
            attribute: u32,
            value: *const c_void,
            size: u32,
        ) -> i32;
    }

    /// `EnumThreadWindows` callback: returning non-zero continues the walk.
    /// `param` carries the flag, so no shared state is involved.
    unsafe extern "system" fn apply(hwnd: Hwnd, param: isize) -> i32 {
        let value: i32 = if param != 0 { 1 } else { 0 };
        let ptr = &value as *const i32 as *const c_void;
        let size = std::mem::size_of::<i32>() as u32;
        // S_OK is 0; older builds reject attribute 20 and want 19.
        if DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, ptr, size) != 0 {
            DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE_OLD, ptr, size);
        }
        1
    }

    pub fn set_dark_decorations(dark: bool) {
        // Safe: we only enumerate our own thread's windows and hand DWM a
        // stack BOOL of the size we declare.
        unsafe {
            EnumThreadWindows(GetCurrentThreadId(), apply, if dark { 1 } else { 0 });
        }
    }

    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const INVALID_HANDLE_VALUE: Hwnd = -1isize as Hwnd;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;

    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn GetStdHandle(which: u32) -> Hwnd;
        fn SetStdHandle(which: u32, handle: Hwnd) -> i32;
        fn CreateFileA(
            name: *const u8,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: Hwnd,
        ) -> Hwnd;
    }

    /// A standard handle we can actually write to. A GUI-subsystem process
    /// started from Explorer has none (null); one started from a shell that
    /// pipes or redirects has a real one, which must be left alone.
    fn has_handle(which: u32) -> bool {
        let h = unsafe { GetStdHandle(which) };
        !h.is_null() && h != INVALID_HANDLE_VALUE
    }

    /// Point one standard handle at the attached console device.
    fn bind(which: u32, device: &[u8], access: u32) {
        // Safe: `device` is a NUL-terminated literal, and the handle is
        // handed straight to SetStdHandle without being dereferenced here.
        unsafe {
            let h = CreateFileA(
                device.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            );
            if h != INVALID_HANDLE_VALUE && !h.is_null() {
                SetStdHandle(which, h);
            }
        }
    }

    pub fn attach_parent_console() {
        // Valid handles already: a pipe, a redirect, or an inherited console.
        // Rust's `println!` will find them by itself.
        if has_handle(STD_OUTPUT_HANDLE) {
            return;
        }
        // Safe: no arguments, and a failure (no parent console -- the
        // double-clicked case) just means there is nothing to attach to.
        if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
            return;
        }
        // AttachConsole gives us the console but leaves the standard handles
        // as they were, so each one has to be pointed at the device by hand.
        bind(STD_OUTPUT_HANDLE, b"CONOUT$\0", GENERIC_WRITE);
        bind(STD_ERROR_HANDLE, b"CONOUT$\0", GENERIC_WRITE);
        bind(STD_INPUT_HANDLE, b"CONIN$\0", GENERIC_READ);
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn set_dark_decorations(_dark: bool) {}
    pub fn attach_parent_console() {}
}
