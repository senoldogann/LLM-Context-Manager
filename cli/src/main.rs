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
    /// Evaluate golden tasks for retrieval quality
    Eval {
        /// Path to golden tasks JSON
        #[arg(short, long, default_value = "eval/golden_tasks.example.json")]
        tasks: PathBuf,

        /// Write JSON report to file (defaults to stdout)
        #[arg(short, long)]
        report: Option<PathBuf>,

        /// Compare structural vs hybrid evaluation
        #[arg(long, alias = "structural")]
        compare: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let env_filter = tracing_subscriber::EnvFilter::try_new(filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    let args = Args::parse();

    match args.cmd {
        Commands::Query { text } => {
            // For CLI query, assume current directory is project root
            let project_path = std::env::current_dir()?.to_string_lossy().to_string();

            match ccm_core::run_query(&text, &project_path).await {
                Ok(results) => {
                    if results.is_empty() {
                        println!("No results found.");
                    } else {
                        for (i, res) in results.iter().enumerate() {
                            println!(
                                "\n#{}: {} (Score: {:.2})",
                                i + 1,
                                res.title,
                                res.relevance_score
                            );
                            println!("Reason: {}", res.reason);
                            println!(
                                "Content Snippet:\n{}\n...",
                                res.content.lines().take(5).collect::<Vec<_>>().join("\n")
                            );
                        }
                    }
                }
                Err(e) => tracing::error!("Query failed: {}", e),
            }
        }
        Commands::Index {
            path,
            db_path,
            watch,
        } => {
            tracing::info!("Starting Indexer CLI");

            let path_str = path.to_string_lossy();
            let db_path_str = db_path.as_ref().map(|p| p.to_string_lossy().to_string());

            // Initial Indexing
            match ccm_core::index_directory(&path_str, db_path_str.as_deref()).await {
                Ok(stats) => {
                    tracing::info!(
                        indexed = stats.files_indexed,
                        failed = stats.files_failed,
                        skipped = stats.files_skipped,
                        nodes = stats.nodes_created,
                        "Initial indexing complete"
                    );
                    if !stats.reason_counts.is_empty() {
                        for (reason, count) in &stats.reason_counts {
                            tracing::info!(reason = %reason, count = *count, "Index issue summary");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Initial indexing failed: {}", e);
                    if !watch {
                        std::process::exit(1);
                    }
                }
            }

            if watch {
                tracing::info!(path = %path.display(), "Watching for changes");
                use notify::{RecursiveMode, Watcher};
                use std::time::Duration;
                use tokio::sync::mpsc;

                let (tx, mut rx) = mpsc::channel(1);

                // Create a channel to receive the events.
                let mut watcher =
                    notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                        match res {
                            Ok(event) => {
                                // Filter out ignored directories and relevant extensions
                                let is_relevant = event.paths.iter().any(|p| {
                                    // Skip common ignored directories
                                    for component in p.components() {
                                        let s = component.as_os_str().to_string_lossy();
                                        if s == "data"
                                            || s == ".ccm"
                                            || s == "node_modules"
                                            || s == "target"
                                            || s == ".git"
                                            || s == ".agent"
                                        {
                                            return false;
                                        }
                                    }

                                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                                        matches!(
                                            ext,
                                            "rs" | "py"
                                                | "ts"
                                                | "js"
                                                | "tsx"
                                                | "jsx"
                                                | "md"
                                                | "json"
                                                | "yaml"
                                                | "yml"
                                                | "toml"
                                        )
                                    } else {
                                        false
                                    }
                                });

                                if is_relevant {
                                    let _ = tx.blocking_send(());
                                }
                            }
                            Err(e) => tracing::error!("Watch error: {:?}", e),
                        }
                    })?;

                // Add a path to be watched. All files and directories at that path and
                // below will be monitored for changes.
                watcher.watch(&path, RecursiveMode::Recursive)?;

                // Debounce loop
                while (rx.recv().await).is_some() {
                    // Flush any other events that came properly
                    while rx.try_recv().is_ok() {}

                    tracing::info!("Change detected. Waiting for settle...");
                    tokio::time::sleep(Duration::from_secs(2)).await;

                    // Flush again
                    while rx.try_recv().is_ok() {}

                    tracing::info!("Re-indexing...");
                    if let Err(e) = ccm_core::update_index(&path_str, db_path_str.as_deref()).await
                    {
                        tracing::warn!(
                            "Incremental indexing failed: {}. Falling back to full index.",
                            e
                        );
                        let _ = ccm_core::index_directory(&path_str, db_path_str.as_deref()).await;
                    }
                }
            }
        }
        Commands::Eval {
            tasks,
            report,
            compare,
        } => {
            if compare {
                let report_data = ccm_core::eval::evaluate_comparison_from_path(&tasks).await?;
                if let Some(path) = report {
                    let file = std::fs::File::create(&path)?;
                    let writer = std::io::BufWriter::new(file);
                    ccm_core::eval::write_comparison_report(writer, &report_data)?;
                    println!("Report written to {}", path.display());
                    let summary = ccm_core::eval::summarize_comparison_report(
                        &report_data,
                        Some(&path.to_string_lossy()),
                    );
                    println!("{}", summary);
                } else {
                    let stdout = std::io::stdout();
                    let handle = stdout.lock();
                    ccm_core::eval::write_comparison_report(handle, &report_data)?;
                    println!();
                }
            } else {
                let report_data = ccm_core::eval::evaluate_from_path(&tasks).await?;
                if let Some(path) = report {
                    let file = std::fs::File::create(&path)?;
                    let writer = std::io::BufWriter::new(file);
                    ccm_core::eval::write_report(writer, &report_data)?;
                    println!("Report written to {}", path.display());
                    let summary = ccm_core::eval::summarize_report(
                        &report_data,
                        Some(&path.to_string_lossy()),
                    );
                    println!("{}", summary);
                } else {
                    let stdout = std::io::stdout();
                    let handle = stdout.lock();
                    ccm_core::eval::write_report(handle, &report_data)?;
                    println!();
                }
            }
        }
    }

    Ok(())
}
