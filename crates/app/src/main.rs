#![forbid(unsafe_code)]

use std::{
    env,
    io::{self, IsTerminal, Write},
    process::ExitCode,
};

use unisphere_cli::CliContext;
use unisphere_sdk::{Inspector, StdConfigReader};

mod adapters;

fn main() -> ExitCode {
    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            let _ = writeln!(
                io::stderr().lock(),
                "unisphere: cannot determine the working directory"
            );
            return ExitCode::FAILURE;
        }
    };
    let stdout = io::stdout();
    let stderr = io::stderr();
    let context = CliContext {
        cwd,
        stdout_is_terminal: stdout.is_terminal(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let args: Vec<_> = env::args_os().collect();
    if args
        .get(1)
        .is_some_and(|arg| arg == "sessions" || arg == "adapters")
    {
        return ExitCode::from(adapters::run(
            args,
            &context,
            &mut stdout.lock(),
            &mut stderr.lock(),
        ));
    }
    let inspector = Inspector::new(StdConfigReader);
    ExitCode::from(unisphere_cli::run(
        args,
        &context,
        &inspector,
        &mut stdout.lock(),
        &mut stderr.lock(),
    ))
}
