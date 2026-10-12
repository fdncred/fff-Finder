// The package keeps the repository's `fffFinder` name.
#![allow(non_snake_case)]

use fff_finder_app::args;

fn main() {
    match args::parse(std::env::args_os().skip(1)) {
        Ok(args::Parsed::Run(args)) => fff_finder_app::run(args),
        Ok(args::Parsed::Help) => print!("{}", args::USAGE),
        Ok(args::Parsed::Version) => println!("fffFinder {}", env!("CARGO_PKG_VERSION")),
        Err(err) => {
            eprintln!("fffFinder: {err}\n\n{}", args::USAGE);
            std::process::exit(2);
        }
    }
}
