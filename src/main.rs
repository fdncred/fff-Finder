// The binary is named `fff-Finder`, which isn't snake case.
#![allow(non_snake_case)]

use fff_Finder::args;

fn main() {
    match args::parse(std::env::args_os().skip(1)) {
        Ok(args::Parsed::Run(args)) if args.detach && !args.pick => detach(),
        Ok(args::Parsed::Run(args)) => {
            // Before GPUI, or any other thread, starts.
            fff_Finder::graphics::prepare(args.software_rendering);
            fff_Finder::run(args)
        }
        Ok(args::Parsed::Help) => print!("{}", args::USAGE),
        Ok(args::Parsed::Version) => println!("fff-Finder {}", env!("CARGO_PKG_VERSION")),
        Err(err) => {
            eprintln!("fff-Finder: {err}\n\n{}", args::USAGE);
            std::process::exit(2);
        }
    }
}

/// Run again without `--detach`, in a process group of its own with no
/// terminal attached, so the window outlives the shell that started it.
fn detach() {
    let me = std::env::current_exe().unwrap_or_else(|_| "fff-Finder".into());
    let args = std::env::args_os().skip(1).filter(|a| a != "--detach");
    let mut command = std::process::Command::new(me);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    if let Err(err) = command.spawn() {
        eprintln!("fff-Finder: could not start: {err}");
        std::process::exit(1);
    }
}
