//! Opt-in hang reports. With `FFF_FINDER_WATCHDOG=1`, a thread watches a
//! heartbeat from the UI thread. When the UI stops responding for three
//! seconds it saves every thread's stack (with `sample` on macOS) in the data
//! folder and prints where.

use gpui_kit::App;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

const STALL: Duration = Duration::from_secs(3);

pub fn start_if_enabled(cx: &mut App) {
    match std::env::var("FFF_FINDER_WATCHDOG").or_else(|_| std::env::var("FFFFINDER_WATCHDOG")) {
        Ok(value) if !value.is_empty() && value != "0" => start(cx),
        _ => {}
    }
}

pub fn start(cx: &mut App) {
    let started = Instant::now();
    let beat = Arc::new(AtomicU64::new(0));
    let ui_beat = beat.clone();
    cx.spawn(async move |cx| {
        loop {
            ui_beat.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
        }
    })
    .detach();
    let spawned = std::thread::Builder::new()
        .name("fff-watchdog".into())
        .spawn(move || {
            let mut reported = None;
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let last = beat.load(Ordering::Relaxed);
                let stalled = (started.elapsed().as_millis() as u64).saturating_sub(last);
                if stalled < STALL.as_millis() as u64 || reported == Some(last) {
                    continue;
                }
                reported = Some(last);
                report(Duration::from_millis(stalled));
            }
        });
    if let Err(err) = spawned {
        eprintln!("fff-Finder: could not start the watchdog: {err}");
    }
}

fn report(stalled: Duration) {
    let pid = std::process::id();
    eprintln!(
        "fff-Finder: the window has not responded for {:.1} s",
        stalled.as_secs_f32()
    );
    match sample(pid) {
        Some(path) => eprintln!("fff-Finder: thread stacks saved to {}", path.display()),
        None => {
            eprintln!("fff-Finder: attach a debugger to process {pid} to see where it is stuck")
        }
    }
}

/// Record the stacks of `pid` with macOS's `sample`.
fn sample(pid: u32) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let dir = fff_Finder_core::config::data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let path = dir.join(format!("hang-{stamp}.txt"));
    let status = std::process::Command::new("/usr/bin/sample")
        .arg(pid.to_string())
        .arg("3")
        .arg("-file")
        .arg(&path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    status.success().then_some(path)
}
