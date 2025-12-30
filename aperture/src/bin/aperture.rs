use std::env;
use std::sync::LazyLock;

use facet::Facet;
use facet_args::HelpConfig;

#[derive(Facet)]
struct Args {
    #[facet(facet_args::subcommand)]
    command: Command,
}

#[derive(Facet)]
#[repr(u8)]
enum Command {
    Version,
    Run,
}

static HELP_CONFIG: LazyLock<HelpConfig> = LazyLock::new(|| HelpConfig {
    program_name: Some(env!("CARGO_PKG_NAME").to_owned()),
    version: Some(env!("CARGO_PKG_VERSION").to_owned()),
    description: Some(env!("CARGO_PKG_DESCRIPTION").to_owned()),
    width: 80,
});

fn parse_args() -> Result<Args, miette::Report> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    // I feel like this shouldn't be necessary...
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    match facet_args::from_slice_with_config(&args, &HELP_CONFIG) {
        Ok(args) => Ok(args),
        Err(err) => Err(err.into()),
    }
}

fn main() -> Result<(), miette::Report> {
    let args = parse_args()?;
    match args.command {
        Command::Version => {
            // let client = ApertureV1Client::new(session);
        }
        Command::Run => {
            let server = aperture::Server::new();
            // TODO
        }
    }
    Ok(())
}
