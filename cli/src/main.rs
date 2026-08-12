use anyhow::Context;
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

        /// Minimum scored-task pass rate required for success
        #[arg(long, default_value_t = 0.0)]
        min_pass_rate: f64,

        /// Previous evaluation report used for regression detection
        #[arg(long)]
        baseline: Option<PathBuf>,

        /// Maximum allowed pass-rate regression in percentage points
        #[arg(long, default_value_t = 0.0)]
        max_regression: f64,

        /// Run evaluation with a specific retrieval policy JSON
        #[arg(long)]
        policy: Option<PathBuf>,
    },
    /// Learn: deterministic synthetic fixture generation and policy optimization
    Learn {
        #[command(subcommand)]
        command: LearnCommand,
    },
    /// Diagnose installation, index compatibility, and provider configuration
    Doctor {
        /// Project root to inspect
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Parser, Debug)]
enum LearnCommand {
    /// Generate synthetic repos, golden tasks, and embedding fixture
    Fixtures {
        /// Output directory (defaults to eval/fixtures)
        #[arg(long, default_value = "eval/fixtures")]
        out: PathBuf,
    },
    /// Optimize a retrieval policy on train tasks and gate it on holdout
    Optimize {
        /// Golden tasks JSON (defaults to the synthetic fixture corpus)
        #[arg(long, default_value = "eval/fixtures/golden_tasks.synthetic.json")]
        tasks: PathBuf,

        /// Report output directory (defaults to eval/fixtures/learn)
        #[arg(long, default_value = "eval/fixtures/learn")]
        out: PathBuf,

        /// Fixed RNG seed (default 42)
        #[arg(long, default_value_t = 42)]
        seed: u64,

        /// Maximum number of candidate evaluations on train
        #[arg(long, default_value_t = 60)]
        max_candidates: usize,
    },
    /// Print a saved learning report
    Report {
        /// Report JSON path
        #[arg(long, default_value = "eval/fixtures/learn/report.json")]
        path: PathBuf,
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

            // First run builds the full index; later runs only apply changed paths.
            match ccm_core::update_index(&path_str, db_path_str.as_deref()).await {
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

                // İzleme filtresi indexleyici politikasıyla tek kaynaktan yönetilir;
                // uzantı/dizin listeleri ayrı yerde tutulmaz (DRY).
                let watch_root = path.clone();
                let mut watcher =
                    notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                        match res {
                            Ok(event) => {
                                let is_relevant = event.paths.iter().any(|p| {
                                    // Araç durum dizinleri indexleyici kapsamı dışında
                                    let skipped_dir = p.components().any(|component| {
                                        matches!(
                                            component.as_os_str().to_string_lossy().as_ref(),
                                            ".ccm" | ".agent"
                                        )
                                    });
                                    !skipped_dir && ccm_core::is_index_relevant_file(&watch_root, p)
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
            min_pass_rate,
            baseline,
            max_regression,
            policy,
        } => {
            if let Some(policy_path) = policy {
                if min_pass_rate > 0.0 || baseline.is_some() || max_regression > 0.0 {
                    anyhow::bail!(
                        "--policy ile --min-pass-rate/--baseline/--max-regression birlikte \
                         kullanılamaz; policy evaluation'ı quality gate uygulamaz"
                    );
                }
                let file = std::fs::File::open(&policy_path)?;
                let policy: ccm_core::policy::RetrievalPolicy =
                    serde_json::from_reader(std::io::BufReader::new(file))?;
                let tasks_file = ccm_core::eval::load_tasks(&tasks)?;
                let report_data = ccm_core::eval::evaluate_policy(tasks_file, &policy).await?;
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
                return Ok(());
            }
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
                let baseline_report = baseline
                    .as_deref()
                    .map(ccm_core::eval::load_report)
                    .transpose()?;
                ccm_core::eval::enforce_quality_gate(
                    &report_data.hybrid,
                    min_pass_rate,
                    baseline_report.as_ref(),
                    max_regression,
                )?;
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
                let baseline_report = baseline
                    .as_deref()
                    .map(ccm_core::eval::load_report)
                    .transpose()?;
                ccm_core::eval::enforce_quality_gate(
                    &report_data,
                    min_pass_rate,
                    baseline_report.as_ref(),
                    max_regression,
                )?;
            }
        }
        Commands::Learn { command } => match command {
            LearnCommand::Fixtures { out } => {
                let generated = ccm_core::fixtures::generate_all(&out).await?;
                println!(
                    "Synthetic fixture generated: {} tasks, {} doc vectors, {} query vectors",
                    generated.task_count, generated.doc_vector_count, generated.query_vector_count
                );
                println!(
                    "Output: {}/golden_tasks.synthetic.json, {}/embeddings.ndjson",
                    out.display(),
                    out.display()
                );
            }
            LearnCommand::Optimize {
                tasks,
                out,
                seed,
                max_candidates,
            } => {
                let tasks_file = ccm_core::eval::load_tasks(&tasks)?;
                let report = ccm_core::optimize::run_learning_pipeline(
                    &tasks_file,
                    &out,
                    seed,
                    max_candidates,
                )
                .await?;
                println!(
                    "Learning pipeline completed: {} candidates evaluated (seed {}), winner v{}",
                    report.candidate_count, report.seed, report.winner_version
                );
                println!(
                    "Holdout recall@k: baseline {:.3} -> winner {:.3}",
                    report.holdout.baseline.mean_recall_at_k,
                    report.holdout.winner.mean_recall_at_k
                );
                println!(
                    "Holdout tokens/task: baseline {:.0} -> winner {:.0}",
                    report.holdout.baseline.mean_tokens, report.holdout.winner.mean_tokens
                );
                println!("Decision: {}", report.decision.reason);
                println!("Report: {}/report.json", out.display());
                if let Some(overfit) = &report.decision.overfit_warning {
                    println!("Overfit flag: {}", overfit);
                }
            }
            LearnCommand::Report { path } => {
                let content = std::fs::read_to_string(&path)?;
                let report: ccm_core::optimize::LearningReport = serde_json::from_str(&content)?;
                println!("Baseline vs Learned (claim: {})", report.claim);
                println!("{:<24} {:>14} {:>14}", "", "Baseline", "Learned");
                let row = |label: &str, pair: &ccm_core::optimize::TrainHoldoutPair| {
                    println!(
                        "{:<24} {:>13.3} {:>13.3}",
                        label, pair.baseline.mean_recall_at_k, pair.winner.mean_recall_at_k
                    );
                };
                println!("Holdout metrics:");
                row("Recall@K", &report.holdout);
                println!(
                    "{:<24} {:>13.1} {:>13.1}",
                    "Tokens/task",
                    report.holdout.baseline.mean_tokens,
                    report.holdout.winner.mean_tokens
                );
                println!(
                    "{:<24} {:>13.1}% {:>13.1}%",
                    "Pass rate", report.holdout.baseline.pass_rate, report.holdout.winner.pass_rate
                );
                println!(
                    "{:<24} {:>13.3} {:>13.3}",
                    "Precision@K",
                    report.holdout.baseline.mean_precision_at_k,
                    report.holdout.winner.mean_precision_at_k
                );
                println!(
                    "{:<24} {:>13.1} {:>13.1}",
                    "Latency ms",
                    report.holdout.baseline.mean_latency_ms,
                    report.holdout.winner.mean_latency_ms
                );
                println!("Decision: {}", report.decision.reason);
                if let Some(secondary) = &report.secondary {
                    println!();
                    println!(
                        "Secondary (real repo, {} tasks, structural-only — iddia taşımaz):",
                        secondary.task_count
                    );
                    println!("{:<24} {:>14} {:>14}", "", "Baseline", "Learned");
                    println!(
                        "{:<24} {:>13.3} {:>13.3}",
                        "Recall@K",
                        secondary.baseline.mean_recall_at_k,
                        secondary.winner.mean_recall_at_k
                    );
                    println!(
                        "{:<24} {:>13.1} {:>13.1}",
                        "Tokens/task", secondary.baseline.mean_tokens, secondary.winner.mean_tokens
                    );
                    println!(
                        "{:<24} {:>13.1}% {:>13.1}%",
                        "Pass rate", secondary.baseline.pass_rate, secondary.winner.pass_rate
                    );
                }
            }
        },
        Commands::Doctor { path, json } => run_doctor(&path, json)?,
    }

    Ok(())
}

fn run_doctor(path: &std::path::Path, json: bool) -> anyhow::Result<()> {
    let root = path.canonicalize()?;
    let data = root.join("data");
    let manifest_path = data.join("ccm_manifest.json");
    let graph_path = data.join("ccm_graph.json");
    let db_path = data.join("ccm_db");

    let manifest_schema = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .and_then(|value| value.get("schema_version").and_then(|v| v.as_u64()));
    let expected_schema = ccm_core::INDEX_SCHEMA_VERSION as u64;
    // MCP sunucusuyla aynı varsayılan: allowlist strict modu varsayılan olarak açık.
    let strict_roots = std::env::var("CCM_REQUIRE_ALLOWED_ROOTS")
        .or_else(|_| std::env::var("CCM_MCP_REQUIRE_ALLOWED_ROOTS"))
        .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(true);
    let allowed_roots = std::env::var("CCM_ALLOWED_ROOTS").ok();
    // MCP ile aynı davranış: CCM_ALLOWED_ROOTS boşsa CCM_PROJECT_ROOT zımni
    // izinli kök olarak kabul edilir.
    let effective_allowed_roots = allowed_roots
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("CCM_PROJECT_ROOT").ok());
    let allowed_roots_ok =
        allowed_roots_check(&root, strict_roots, effective_allowed_roots.as_deref());
    let provider = std::env::var("CCM_EMBEDDING_PROVIDER")
        .or_else(|_| std::env::var("EMBEDDING_PROVIDER"))
        .unwrap_or_else(|_| "local".to_string());
    let model = std::env::var("CCM_EMBEDDING_MODEL")
        .or_else(|_| std::env::var("EMBEDDING_MODEL"))
        .unwrap_or_else(|_| "default".to_string());

    let checks = serde_json::json!({
        "project_root": {"ok": root.is_dir(), "value": root},
        "allowed_roots": {
            "ok": allowed_roots_ok,
            "strict": strict_roots,
            "value": allowed_roots
        },
        "manifest": {
            "ok": manifest_schema == Some(expected_schema),
            "path": manifest_path,
            "schema": manifest_schema,
            "expected_schema": expected_schema
        },
        "graph": {"ok": graph_path.is_file(), "path": graph_path},
        "vector_index": {"ok": db_path.is_dir(), "path": db_path},
        "embedding": {"ok": !provider.trim().is_empty(), "provider": provider, "model": model},
        "binary": {"ok": true, "version": env!("CARGO_PKG_VERSION")}
    });

    let healthy = checks
        .as_object()
        .context("doctor checks must be an object")?
        .values()
        .all(|check| check.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
    let output = serde_json::json!({"healthy": healthy, "checks": checks});

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "CCM doctor: {}",
            if healthy { "healthy" } else { "issues found" }
        );
        let checks = output["checks"].as_object().context("checks object")?;
        for (name, check) in checks {
            let ok = check.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            println!("{} {}", if ok { "✓" } else { "✗" }, name);
        }
    }

    if !healthy {
        anyhow::bail!("doctor found configuration or index issues");
    }
    Ok(())
}

fn allowed_roots_check(root: &std::path::Path, strict: bool, raw: Option<&str>) -> bool {
    if !strict {
        return true;
    }
    let Some(raw) = raw.filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    let separators: &[char] = if cfg!(windows) {
        &[';', ',']
    } else {
        &[':', ';', ',']
    };
    raw.split(separators)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(std::path::Path::new)
        .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
        .any(|allowed| root.starts_with(allowed))
}

#[cfg(test)]
mod doctor_tests {
    use super::allowed_roots_check;

    #[test]
    fn optional_allowlist_is_healthy_when_not_configured() {
        assert!(allowed_roots_check(
            std::path::Path::new("/tmp/project"),
            false,
            None
        ));
    }

    #[test]
    fn strict_allowlist_requires_the_project_root() {
        let root = std::path::Path::new("/tmp/projects/app");
        assert!(allowed_roots_check(root, true, Some("/tmp/projects")));
        assert!(!allowed_roots_check(root, true, Some("/tmp/other")));
        assert!(!allowed_roots_check(root, true, None));
    }
}
