use ccm_core::server;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Start the CCM server
    Start,
    /// Query the knowledge base
    Query {
        #[arg(short, long)]
        text: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.cmd {
        Commands::Start => {
            println!("Starting CCM Server...");
            server::start_server().await?;
        }
        Commands::Query { text } => {
            println!("Querying for: {}", text);
            // TODO: Implement actual query logic calling core
        }
    }

    Ok(())
}
