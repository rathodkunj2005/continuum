//! Screen Recording permission probe (macOS).

/// Returns whether Screen Recording is likely allowed for this process.
/// Uses `CGPreflightScreenCaptureAccess` when available (macOS 10.15+ SDK);
/// otherwise reports unknown and relies on capture frame counts.
#[cfg(target_os = "macos")]
pub fn preflight_screen_capture_access() -> (bool, String) {
    type Preflight = unsafe extern "C" fn() -> bool;
    const CG_PATH: &[u8] = b"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics\0";
    const SYM: &[u8] = b"CGPreflightScreenCaptureAccess\0";
    static PRECHECK: std::sync::OnceLock<Result<Preflight, String>> = std::sync::OnceLock::new();

    let preflight = PRECHECK.get_or_init(|| unsafe {
        let handle = libc::dlopen(CG_PATH.as_ptr() as *const libc::c_char, libc::RTLD_LAZY);
        if handle.is_null() {
            return Err(
                "Could not load CoreGraphics; assume permission OK until capture runs."
                    .to_string(),
            );
        }
        let sym = libc::dlsym(handle, SYM.as_ptr() as *const libc::c_char);
        if sym.is_null() {
            libc::dlclose(handle);
            return Err(
                "CGPreflightScreenCaptureAccess not available; check frame count or System Settings → Privacy & Security → Screen Recording.".to_string(),
            );
        }
        let f: Preflight = std::mem::transmute(sym);
        // Keep the framework handle alive for process lifetime to keep `f` valid.
        Ok(f)
    });

    match preflight {
        Ok(f) => unsafe {
            let ok = f();
            let detail = if ok {
                "Screen Recording access preflight OK.".to_string()
            } else {
                "Open System Settings → Privacy & Security → Screen Recording → enable Continuum, then restart the app.".to_string()
            };
            (ok, detail)
        },
        Err(detail) => (true, detail.clone()),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn preflight_screen_capture_access() -> (bool, String) {
    (
        true,
        "Screen capture not applicable on this platform.".to_string(),
    )
}
