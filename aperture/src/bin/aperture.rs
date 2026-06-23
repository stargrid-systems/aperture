use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use miette::IntoDiagnostic;

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
    /// Pre-download components (the Spectra frontend) for offline use.
    Prefetch(PrefetchArgs),
    /// Print the OpenAPI specification as JSON.
    Openapi,
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
struct PrefetchArgs {
    /// Directory for runtime data and cached components.
    #[arg(long, env = "APERTURE_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            let core = aperture_core::Core::new();
            println!("aperture {}", core.version().aperture);
            Ok(())
        }
        Command::Openapi => {
            let doc = aperture::openapi();
            let json = serde_json::to_string_pretty(&doc).into_diagnostic()?;
            println!("{json}");
            Ok(())
        }
        Command::Run(args) => {
            init_tracing();
            block_on(aperture::serve(args.addr, args.data_dir))
        }
        Command::Prefetch(args) => {
            init_tracing();
            block_on(aperture::prefetch(args.data_dir))
        }
    }
}

fn block_on<F: Future<Output = miette::Result<()>>>(future: F) -> miette::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .into_diagnostic()?;
    runtime.block_on(future)
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
