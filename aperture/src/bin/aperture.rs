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
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Address to bind the HTTPS server.
    #[arg(long, env = "APERTURE_ADDR", default_value = "[::1]:8000")]
    addr: SocketAddr,
    /// Address for the HTTP listener (redirects to HTTPS by default).
    #[arg(long, env = "APERTURE_HTTP_ADDR", default_value = "[::1]:8080")]
    http_addr: Option<SocketAddr>,
    /// Serve the full API over plain HTTP instead of redirecting to HTTPS.
    /// Intended for certificate recovery only.
    #[arg(long, env = "APERTURE_INSECURE_HTTP", default_value_t = false)]
    insecure_http: bool,
    /// Directory for runtime data and cached components.
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
        Command::Run(args) => block_on(aperture::serve(
            args.addr,
            args.http_addr,
            args.insecure_http,
            args.data_dir,
        )),
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
