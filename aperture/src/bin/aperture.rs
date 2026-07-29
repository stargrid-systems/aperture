use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

use clap::{Args, Parser, Subcommand};
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
    /// Address to bind the HTTPS server.
    /// Pass an empty string to disable HTTPS entirely (recovery mode).
    #[arg(long, env = "APERTURE_HTTPS_ADDR", default_value = "[::1]:8443")]
    https_addr: BindAddr,
    /// Address for the HTTP listener.
    /// Redirects to HTTPS when HTTPS is enabled, otherwise serves the full API.
    /// Pass an empty string to disable the HTTP listener entirely.
    #[arg(long, env = "APERTURE_HTTP_ADDR", default_value = "[::1]:8080")]
    http_addr: BindAddr,
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

/// Listener address.
///
/// An empty string means "no listener". Anything else is parsed strictly as a
/// `SocketAddr` (`host:port`), so hostnames like `localhost:8080` are not
/// supported. Use an IP literal such as `127.0.0.1:8080` or
/// `[::1]:8080` instead.
#[derive(Clone, Debug)]
struct BindAddr(Option<SocketAddr>);

impl FromStr for BindAddr {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        if s.is_empty() {
            return Ok(BindAddr(None));
        }
        s.parse::<SocketAddr>()
            .map(Some)
            .map(BindAddr)
            .map_err(|err| {
                anyhow::Error::from(err).context(format!("invalid socket address {s:?}"))
            })
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            println!("aperture {}", aperture::VERSION);
            Ok(())
        }
        Command::Openapi => block_on(emit_openapi()),
        Command::Run(args) => block_on(aperture::serve(
            args.https_addr.0,
            args.http_addr.0,
            args.data_dir,
        )),
        Command::ResetPassword(args) => {
            block_on(aperture::reset_password(&args.user, &args.data_dir))
        }
    }
}

async fn emit_openapi() -> anyhow::Result<()> {
    let doc = aperture::openapi().await?;
    let json = serde_json::to_string_pretty(&doc)?;
    println!("{json}");
    Ok(())
}

fn block_on<F: Future<Output = anyhow::Result<()>>>(future: F) -> anyhow::Result<()> {
    let runtime = runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(future)
}
