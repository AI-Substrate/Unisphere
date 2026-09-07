#![forbid(unsafe_code)]

use std::{
    env,
    io::{self, IsTerminal, Write},
    process::ExitCode,
};

use unisphere_cli::CliContext;
use unisphere_sdk::{Inspector, StdConfigReader};

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
    let inspector = Inspector::new(StdConfigReader);
    ExitCode::from(unisphere_cli::run(
        env::args_os(),
        &context,
        &inspector,
        &mut stdout.lock(),
        &mut stderr.lock(),
    ))
}
