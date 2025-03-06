use camino::Utf8PathBuf;
use std::{
    io::{stdin, Read},
    process::ExitCode,
};

mod scripting;

#[derive(Debug, clap::Parser)]
struct Cli {
    script: Utf8PathBuf,
    #[clap(subcommand)]
    subcommand: Subcommand,
}

#[derive(Clone, Debug, clap::Subcommand)]
enum Subcommand {
    FromStdin,
    FromArgs { args: Vec<String> },
}

struct AlreadyReportedToCommandLine;

fn handle<T>(res: Result<T, AlreadyReportedToCommandLine>) -> Result<T, ExitCode> {
    res.map_err(|AlreadyReportedToCommandLine| ExitCode::FAILURE)
}

fn main() -> Result<(), ExitCode> {
    let Cli { script, subcommand } = clap::Parser::parse();

    let input = match subcommand {
        Subcommand::FromStdin => {
            let mut buf = String::new();
            stdin().lock().read_to_string(&mut buf).unwrap();
            buf.lines().map(|s| s.to_owned()).collect::<Vec<_>>()
        }
        Subcommand::FromArgs { args } => args,
    };

    let mut engine = handle(scripting::ScriptingEngine::new(script))?;
    for input in input {
        let rendered = match input.parse() {
            Ok(url) => handle(engine.run(url))?,
            Err(_e) => input,
        };
        println!("{rendered}");
    }
    Ok(())
}
