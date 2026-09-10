//! Native window decoration, where the toolkit does not handle it.
//!
//! Windows draws the caption bar itself, outside the Qt scene, and Qt 6 does
//! not ask DWM for a dark one — a fully dark app therefore gets a white title
//! bar on a dark desktop. `DWMWA_USE_IMMERSIVE_DARK_MODE` is the only knob for
//! it, so we set it on our own top-level windows.
//!
//! On Linux the compositor owns the decoration and follows the desktop
//! preference, so there is nothing to do; the function is a no-op.

/// Ask the window manager for dark (or light) decorations on every top-level
/// window of the calling thread — the GUI thread, where Qt creates them.
pub fn set_dark_decorations(dark: bool) {
    imp::set_dark_decorations(dark);
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
}

#[cfg(not(windows))]
mod imp {
    pub fn set_dark_decorations(_dark: bool) {}
}
