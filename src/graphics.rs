//! Drawing without a GPU. On Linux, GPUI uses the first graphics adapter
//! that can draw the window, real GPUs before Mesa's software renderers, so
//! a machine with no usable GPU (WSL2, many virtual machines) still works,
//! after Mesa prints warnings about each hardware driver it tried.

use gpui_kit::Window;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Why fff-Finder asked Mesa for software rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// `--software-rendering`.
    Flag,
    /// `FFF_FINDER_SOFTWARE_RENDERING`.
    Env,
    /// The system shows no GPU device, so no hardware driver could work.
    NoGpuDevice,
}

static CHOSEN: OnceLock<Option<Reason>> = OnceLock::new();

/// Whether to ask for software rendering: the flag forces it, the variable
/// forces it unless it's `0`, which turns it off, and otherwise it's on only
/// when there's no GPU device.
pub fn decide(flag: bool, env: Option<&str>, gpu_device: bool) -> Option<Reason> {
    match env.filter(|value| !value.is_empty()) {
        _ if flag => Some(Reason::Flag),
        Some("0") => None,
        Some(_) => Some(Reason::Env),
        None => (!gpu_device).then_some(Reason::NoGpuDevice),
    }
}

/// On Linux, when [`decide`] says so, point Mesa straight at its software
/// renderers, llvmpipe for OpenGL and lavapipe for Vulkan, so it doesn't try
/// the hardware drivers. Values already in the environment win. Call it
/// before any other thread starts.
pub fn prepare(flag: bool) {
    if !cfg!(all(unix, not(target_os = "macos"))) {
        return;
    }
    let env = std::env::var("FFF_FINDER_SOFTWARE_RENDERING").ok();
    let reason = decide(flag, env.as_deref(), has_gpu_device());
    CHOSEN.get_or_init(|| reason);
    if reason.is_none() {
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

/// Whether a GPU driver could work at all: any DRM device, WSL's GPU device,
/// or NVIDIA's driver. It errs toward yes, since a wrong no would give up a
/// working GPU.
fn has_gpu_device() -> bool {
    let any_entry = |dir: &str| std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some());
    let nvidia = std::fs::read_dir("/dev").is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("nvidia"))
    });
    any_entry("/dev/dri") || Path::new("/dev/dxg").exists() || nvidia
}

/// Say once on stderr when windows are drawn on the CPU, and why.
pub fn report(window: &Window) {
    static REPORTED: AtomicBool = AtomicBool::new(false);
    let Some(gpu) = window.gpu_specs().filter(|gpu| gpu.is_software_emulated) else {
        return;
    };
    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let device = gpu.device_name;
    match CHOSEN.get().copied().flatten() {
        Some(Reason::Flag | Reason::Env) => {
            eprintln!("fff-Finder: drawing in software with {device}, as asked")
        }
        Some(Reason::NoGpuDevice) => {
            eprintln!("fff-Finder: no GPU device found; drawing in software with {device}")
        }
        None => eprintln!(
            "fff-Finder: no GPU driver worked; drawing in software with {device}. \
             Pass --software-rendering or set FFF_FINDER_SOFTWARE_RENDERING=1 \
             to skip the GPU drivers and their warnings."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_software_rendering() {
        assert_eq!(decide(true, Some("0"), true), Some(Reason::Flag));
        assert_eq!(decide(false, Some("1"), true), Some(Reason::Env));
        assert_eq!(decide(false, Some("0"), false), None, "0 turns it off");
        assert_eq!(decide(false, None, false), Some(Reason::NoGpuDevice));
        assert_eq!(decide(false, Some(""), false), Some(Reason::NoGpuDevice));
        assert_eq!(decide(false, None, true), None);
    }
}
