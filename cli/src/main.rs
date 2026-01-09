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

        /// Watch for changes and re-index automatically
        #[arg(short, long)]
        watch: bool,
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
        Commands::Index {
            path,
            db_path,
            watch,
        } => {
            println!("╔══════════════════════════════════════╗");
            println!("║     CCM - Codebase Indexer           ║");
            println!("╚══════════════════════════════════════╝");

            let path_str = path.to_string_lossy();
            let db_path_str = db_path.as_ref().map(|p| p.to_string_lossy().to_string());

            // Initial Indexing
            if let Err(e) = ccm_core::index_directory(&path_str, db_path_str.as_deref()).await {
                eprintln!("Initial indexing failed: {}", e);
                if !watch {
                    std::process::exit(1);
                }
            } else {
                println!("\n═══════════════════════════════════════");
                println!("Initial Indexing Complete!");
                println!("═══════════════════════════════════════");
            }

            if watch {
                println!("\n👀 Watching for changes in: {}", path.display());
                use notify::{EventKind, RecursiveMode, Watcher};
                use std::time::Duration;
                use tokio::sync::mpsc;

                let (tx, mut rx) = mpsc::channel(1);

                // Create a channel to receive the events.
                let mut watcher =
                    notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                        match res {
                            Ok(event) => {
                                // Simple filter for interesting extensions
                                let is_relevant = event.paths.iter().any(|p| {
                                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                                        matches!(ext, "rs" | "py" | "ts" | "js" | "tsx" | "jsx")
                                    } else {
                                        false
                                    }
                                });

                                if is_relevant {
                                    let _ = tx.blocking_send(());
                                }
                            }
                            Err(e) => eprintln!("Watch error: {:?}", e),
                        }
                    })?;

                // Add a path to be watched. All files and directories at that path and
                // below will be monitored for changes.
                watcher.watch(&path, RecursiveMode::Recursive)?;

                // Debounce loop
                while let Some(_) = rx.recv().await {
                    // Flush any other events that came properly
                    while rx.try_recv().is_ok() {}

                    println!("\n🔄 Change detected. Waiting for settle...");
                    tokio::time::sleep(Duration::from_secs(2)).await;

                    // Flush again
                    while rx.try_recv().is_ok() {}

                    println!("🚀 Re-indexing...");
                    let _ = ccm_core::index_directory(&path_str, db_path_str.as_deref()).await;
                }
            }
        }
    }

    Ok(())
}
