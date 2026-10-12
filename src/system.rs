//! Handing things to the operating system: open a file with its default
//! app, show it in Finder/Explorer, or start a terminal. Tests install a
//! [`Recorder`] instead, so nothing opens on the machine running them.
//! (GPUI's test platform doesn't implement the first two at all.)

use fff_finder_core::terminals::Plan;
use gpui_kit::{App, AppContext as _, Global, Task};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemCall {
    Open(PathBuf),
    Reveal(PathBuf),
    Terminal { program: PathBuf, args: Vec<String> },
}

/// When set as a global, calls are recorded here and not performed.
#[derive(Clone, Default)]
pub struct Recorder(pub Rc<RefCell<Vec<SystemCall>>>);

impl Global for Recorder {}

impl Recorder {
    pub fn calls(&self) -> Vec<SystemCall> {
        self.0.borrow().clone()
    }
}

pub fn open(path: &Path, cx: &mut App) {
    match cx.try_global::<Recorder>() {
        Some(recorder) => recorder.0.borrow_mut().push(SystemCall::Open(path.into())),
        None => cx.open_with_system(path),
    }
}

/// Start a terminal. The plan may wait on AppleScript (and a permission
/// prompt), so it runs on a background thread; errors come back in the task.
pub fn launch(plan: Plan, cx: &mut App) -> Task<std::io::Result<()>> {
    match cx.try_global::<Recorder>() {
        Some(recorder) => {
            recorder.0.borrow_mut().push(SystemCall::Terminal {
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
        Some(recorder) => recorder
            .0
            .borrow_mut()
            .push(SystemCall::Reveal(path.into())),
        None => cx.reveal_path(path),
    }
}
