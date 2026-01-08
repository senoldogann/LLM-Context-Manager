use ccm_core::server;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "CCM - Cognitive Codebase Matrix CLI", long_about = None)]
struct Args {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Start the CCM server
    Start {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },
    /// Query the knowledge base (semantic search or file:line lookup)
    Query {
        /// Search text or file:line format (e.g., "authentication" or "src/main.rs:50")
        #[arg(short, long)]
        text: String,
    },
    /// Index a directory into the vector database
    Index {
        /// Path to the directory to index
        #[arg(short, long)]
        path: PathBuf,

        /// Custom database path (optional)
        #[arg(short, long)]
        db_path: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.cmd {
        Commands::Start { port } => {
            println!("Starting CCM Server on port {}...", port);
            server::start_server(port).await?;
        }
        Commands::Query { text } => {
            if let Err(e) = ccm_core::run_query(&text).await {
                eprintln!("Query failed: {}", e);
            }
        }
        Commands::Index { path, db_path } => {
            println!("╔══════════════════════════════════════╗");
            println!("║     CCM - Codebase Indexer           ║");
            println!("╚══════════════════════════════════════╝");

            let path_str = path.to_string_lossy();
            let db_path_str = db_path.as_ref().map(|p| p.to_string_lossy().to_string());

            match ccm_core::index_directory(&path_str, db_path_str.as_deref()).await {
                Ok(stats) => {
                    println!("\n═══════════════════════════════════════");
                    println!("Indexing Complete!");
                    println!("  Files indexed: {}", stats.files_indexed);
                    println!("  Files failed:  {}", stats.files_failed);
                    println!("  Nodes created: {}", stats.nodes_created);
                    println!("═══════════════════════════════════════");
                }
                Err(e) => {
                    eprintln!("Indexing failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
