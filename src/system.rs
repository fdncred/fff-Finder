//! Handing things to the operating system: open a file with its default
//! app or a text editor, show it in Finder/Explorer, or start a terminal. Tests install a
//! [`Recorder`] instead, so nothing opens on the machine running them.
//! (GPUI's test platform doesn't implement the first two at all.)

use fffinder_core::terminals::Plan;
use gpui_kit::{App, AppContext as _, Global, Task};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemCall {
    Open(PathBuf),
    /// Opened in the system's text editor (`open -t`, Notepad).
    EditText(PathBuf),
    Reveal(PathBuf),
    Terminal {
        program: PathBuf,
        args: Vec<String>,
    },
}

/// When set as a global, calls are recorded here and not performed.
#[derive(Clone, Default)]
pub struct Recorder {
    calls: Rc<RefCell<Vec<SystemCall>>>,
    /// Files that no app claims.
    no_app: Rc<RefCell<HashSet<PathBuf>>>,
}

impl Global for Recorder {}

impl Recorder {
    pub fn calls(&self) -> Vec<SystemCall> {
        self.calls.borrow().clone()
    }

    /// Act as if no app is registered to open `path`.
    pub fn set_no_app(&self, path: impl Into<PathBuf>) {
        self.no_app.borrow_mut().insert(path.into());
    }

    fn record(&self, call: SystemCall) {
        self.calls.borrow_mut().push(call);
    }
}

pub fn open(path: &Path, cx: &mut App) {
    match cx.try_global::<Recorder>() {
        Some(recorder) => recorder.record(SystemCall::Open(path.into())),
        None => cx.open_with_system(path),
    }
}

/// Whether an app is registered to open the file at `path`; `None` when the
/// system can't tell (Windows asks with its own "Open with" chooser).
pub fn has_default_app(path: &Path, cx: &App) -> Option<bool> {
    if let Some(recorder) = cx.try_global::<Recorder>() {
        return Some(!recorder.no_app.borrow().contains(path));
    }
    #[cfg(target_os = "macos")]
    {
        Some(crate::macos::has_default_app(path))
    }
    #[cfg(windows)]
    {
        None
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        xdg_has_default_app(path)
    }
}

/// Asks `xdg-mime` for the file's type and the app set for that type.
#[cfg(not(any(target_os = "macos", windows)))]
fn xdg_has_default_app(path: &Path) -> Option<bool> {
    let query = |args: &[&std::ffi::OsStr]| {
        let out = std::process::Command::new("xdg-mime")
            .arg("query")
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let mime = query(&["filetype".as_ref(), path.as_os_str()]).filter(|m| !m.is_empty())?;
    Some(!query(&["default".as_ref(), mime.as_ref()])?.is_empty())
}

/// Open `path` in the system's text editor: whatever opens plain text on
/// macOS, Notepad on Windows. `None` where there isn't one (Linux).
pub fn edit_text(path: &Path, cx: &mut App) -> Option<Task<std::io::Result<()>>> {
    if let Some(recorder) = cx.try_global::<Recorder>() {
        recorder.record(SystemCall::EditText(path.into()));
        return Some(Task::ready(Ok(())));
    }
    let path = path.to_path_buf();
    if cfg!(target_os = "macos") {
        Some(cx.background_spawn(async move {
            let out = std::process::Command::new("open")
                .args(["-t", "--"])
                .arg(&path)
                .output()?;
            if out.status.success() {
                Ok(())
            } else {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                Err(std::io::Error::other(err))
            }
        }))
    } else if cfg!(windows) {
        Some(cx.background_spawn(async move {
            std::process::Command::new("notepad")
                .arg(&path)
                .spawn()
                .map(drop)
        }))
    } else {
        None
    }
}

/// Start a terminal. The plan may wait on AppleScript (and a permission
/// prompt), so it runs on a background thread; errors come back in the task.
pub fn launch(plan: Plan, cx: &mut App) -> Task<std::io::Result<()>> {
    match cx.try_global::<Recorder>() {
        Some(recorder) => {
            recorder.record(SystemCall::Terminal {
                program: plan.program.clone(),
                args: plan.args.clone(),
            });
            Task::ready(Ok(()))
        }
        None => cx.background_spawn(async move { plan.run() }),
    }
}

pub fn reveal(path: &Path, cx: &mut App) {
    match cx.try_global::<Recorder>() {
        Some(recorder) => recorder.record(SystemCall::Reveal(path.into())),
        None => cx.reveal_path(path),
    }
}
