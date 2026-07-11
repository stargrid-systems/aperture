use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use miette::IntoDiagnostic;
use tokio::runtime;

/// Stargrid hardware application gateway.
#[derive(Debug, Parser)]
#[command(name = "aperture", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print version information.
    Version,
    /// Run the gateway HTTP server.
    Run(RunArgs),
    /// Print the OpenAPI specification as JSON.
    Openapi,
    /// Reset the password for a user. Prints the new password to stdout.
    ResetPassword(ResetPasswordArgs),
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Address to bind the HTTP server to. Defaults to the IPv6 loopback.
    #[arg(long, env = "APERTURE_ADDR", default_value = "[::1]:8000")]
    addr: SocketAddr,
    /// Directory for runtime data and cached components.
    #[arg(long, env = "APERTURE_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,
}

#[derive(Debug, Args)]
struct ResetPasswordArgs {
    /// Username whose password to reset. Defaults to "admin".
    #[arg(long, default_value = "admin")]
    user: String,
    /// Directory for runtime data.
    #[arg(long, env = "APERTURE_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            println!("aperture {}", aperture::VERSION);
            Ok(())
        }
        Command::Openapi => block_on(emit_openapi()),
        Command::Run(args) => block_on(aperture::serve(args.addr, args.data_dir)),
        Command::ResetPassword(args) => block_on(aperture::reset_password(&args.user, &args.data_dir)),
    }
}

async fn emit_openapi() -> miette::Result<()> {
    let doc = aperture::openapi().await?;
    let json = serde_json::to_string_pretty(&doc).into_diagnostic()?;
    println!("{json}");
    Ok(())
}

fn block_on<F: Future<Output = miette::Result<()>>>(future: F) -> miette::Result<()> {
    let runtime = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;
    runtime.block_on(future)
}
