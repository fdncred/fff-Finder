//! Drawing without a GPU. On Linux, GPUI uses the first graphics adapter
//! that can draw the window, real GPUs before Mesa's software renderers, so
//! a machine with no usable GPU (WSL2, many virtual machines) still works,
//! after Mesa prints warnings about each hardware driver it tried.

use gpui_kit::Window;
use std::sync::atomic::{AtomicBool, Ordering};

/// With `FFF_FINDER_SOFTWARE_RENDERING=1` on Linux, point Mesa straight at
/// its software renderers, llvmpipe for OpenGL and lavapipe for Vulkan, so
/// it doesn't try the hardware drivers. Values already in the environment
/// win. Call it before any other thread starts.
pub fn prepare() {
    let on = std::env::var("FFF_FINDER_SOFTWARE_RENDERING")
        .is_ok_and(|value| !value.is_empty() && value != "0");
    if !on || !cfg!(all(unix, not(target_os = "macos"))) {
        return;
    }
    for (key, value) in [
        ("LIBGL_ALWAYS_SOFTWARE", "1"),
        ("VK_LOADER_DRIVERS_SELECT", "*lvp*"),
    ] {
        if std::env::var_os(key).is_none() {
            // SAFETY: `main` calls this before any other thread exists.
            unsafe { std::env::set_var(key, value) };
        }
    }
}

/// Say once on stderr when windows are drawn on the CPU.
pub fn report(window: &Window) {
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if let Some(gpu) = window.gpu_specs()
        && gpu.is_software_emulated
        && !REPORTED.swap(true, Ordering::Relaxed)
    {
        eprintln!(
            "fff-Finder: drawing in software with {} ({}), not on a GPU",
            gpu.device_name, gpu.driver_name
        );
    }
}
