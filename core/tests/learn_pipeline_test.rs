//! Self-improvement pipeline entegrasyon testi (Phase 1 proof of mechanism).
//!
//! Sentetik fixture üretimi deterministiktir; optimize + gate, production default
//! baseline ile ya Promote ya da Rejected üretir (ikisi de geçerli sonuçtur) ve
//! history'ye yazar. Ayrıca kasıtlı kötü baseline ile harness'in gerçek
//! iyileşmeyi yakalayabildiği (sanity) ve recall kaybettiren policy'nin
//! reddedildiği doğrulanır.

use anyhow::Result;
use ccm_core::eval::{self, GoldenTasksFile};
use ccm_core::fixtures::{copy_source_repos, generate_all, FIXTURE_SEED};
use ccm_core::optimize::{run_learning_pipeline, OptimizationOutcome};
use ccm_core::policy::gate::{evaluate_promotion, PromotionOptions};
use ccm_core::policy::store::PolicyStore;
use ccm_core::policy::RetrievalPolicy;
use tempfile::tempdir;

fn source_repos() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("eval/fixtures/repos")
}

fn fixture_env(embedding_path: &std::path::Path, learn_dir: &std::path::Path) {
    std::env::set_var("CCM_EMBEDDING_FIXTURE", embedding_path);
    std::env::set_var("CCM_LEARN_DIR", learn_dir);
    // İkincil gerçek-repo corpus'u bu testte kurulmaz (tüm core repo indeksini
    // kurmak test süresini şişirir); ikincil tablo CLI/CI akışında ayrıca doğrulanır.
    std::env::set_var(
        "CCM_REAL_EVAL_TASKS",
        learn_dir.join("missing.json").to_string_lossy().to_string(),
    );
    std::env::remove_var("CCM_DISABLE_EMBEDDER");
}

#[tokio::test]
async fn synthetic_corpus_and_pipeline_are_deterministic_and_gate_works() -> Result<()> {
    // 1. Deterministik fixture üretimi: iki üretim birebir aynı olmalı.
    let dir_a = tempdir()?;
    let dir_b = tempdir()?;
    copy_source_repos(&source_repos(), &dir_a.path().join("repos"))?;
    copy_source_repos(&source_repos(), &dir_b.path().join("repos"))?;
    generate_all(dir_a.path()).await?;
    generate_all(dir_b.path()).await?;
    let mut tasks_a: serde_json::Value = serde_json::from_slice(&std::fs::read(
        dir_a.path().join("golden_tasks.synthetic.json"),
    )?)?;
    let mut tasks_b: serde_json::Value = serde_json::from_slice(&std::fs::read(
        dir_b.path().join("golden_tasks.synthetic.json"),
    )?)?;
    for tasks in [&mut tasks_a, &mut tasks_b] {
        for task in tasks["tasks"].as_array_mut().unwrap() {
            task["repo"]["path"] = serde_json::Value::String("<tmp>".to_string());
        }
    }
    assert_eq!(tasks_a, tasks_b, "fixture üretimi deterministik değil");
    let embeddings_a = std::fs::read(dir_a.path().join("embeddings.ndjson"))?;
    let embeddings_b = std::fs::read(dir_b.path().join("embeddings.ndjson"))?;
    assert_eq!(
        embeddings_a, embeddings_b,
        "embedding fixture deterministik değil"
    );

    let out = tempdir()?;
    let learn_dir = tempdir()?;
    copy_source_repos(&source_repos(), &out.path().join("repos"))?;
    generate_all(out.path()).await?;
    let embedding_path = out.path().join("embeddings.ndjson");
    fixture_env(&embedding_path, learn_dir.path());

    // 2. Pipeline: optimize (sınırlı aday) + holdout gate + history.
    let tasks_file = eval::load_tasks(&out.path().join("golden_tasks.synthetic.json"))?;
    let report =
        run_learning_pipeline(&tasks_file, &out.path().join("learn"), FIXTURE_SEED, 8).await?;
    assert!(report.candidate_count >= 1);
    let _ = report.decision.promoted; // Promote ya da Rejected geçerli sonuçtur
    let report_bytes = std::fs::read(out.path().join("learn/report.json"))?;
    assert!(!report_bytes.is_empty());

    let history = std::fs::read_to_string(learn_dir.path().join("history.jsonl"))?;
    assert_eq!(history.lines().count(), 1, "history tek giriş içermeli");
    let store = PolicyStore::load(&PolicyStore::default_policies_path())?;
    assert!(store
        .policies
        .iter()
        .any(|p| p.version == report.winner_version));

    let train = split_tasks(&tasks_file, "train");
    let holdout = split_tasks(&tasks_file, "holdout");

    // 3. Sanity (pozitif): düşük top_k ile kısıtlanmış zayıf baseline'a karşı
    //    geri getirmeyi artıran aday, gate'ten geçer (recall iyileşmesi yolu).
    //    Daha zengin fixture'da semantic hit'ler recall'a katkı sağladığı için
    //    token-efficiency yolu dar; gate'in recall-improvement dalı burada
    //    gösterilir. Token guard, geri getirmeyi artıran adayın gerçekçi ek
    //    yükünü kabul edecek şekilde (2.0x) gevşetilir.
    let mut weak_baseline = RetrievalPolicy::baseline();
    weak_baseline.top_k = 1;
    weak_baseline.semantic_hits = 0;

    let mut good_candidate = RetrievalPolicy::baseline();
    good_candidate.top_k = 3;
    good_candidate.semantic_hits = 1;
    good_candidate.version = 2;

    let weak_train = eval::evaluate_policy(train.clone(), &weak_baseline).await?;
    let good_train = eval::evaluate_policy(train.clone(), &good_candidate).await?;
    let weak_holdout = eval::evaluate_policy(holdout.clone(), &weak_baseline).await?;
    let good_holdout = eval::evaluate_policy(holdout.clone(), &good_candidate).await?;
    let positive_options = PromotionOptions {
        token_guard_ratio: 2.0,
        ..Default::default()
    };
    let decision = evaluate_promotion(
        &weak_train,
        &good_train,
        &weak_holdout,
        &good_holdout,
        &positive_options,
    );
    assert!(
        decision.promoted,
        "geri getirme iyileşmesi promote edilmeli: {}",
        decision.reason
    );

    // 4. Sanity (negatif, varsayılan guard): default baseline'a göre geri getirmeyi
    //    kaybettiren policy reddedilir. Varsayılan `PromotionOptions` kullanılır.
    let default_train = eval::evaluate_policy(train.clone(), &RetrievalPolicy::baseline()).await?;
    let default_holdout =
        eval::evaluate_policy(holdout.clone(), &RetrievalPolicy::baseline()).await?;
    let mut recall_hurting = RetrievalPolicy::baseline();
    recall_hurting.semantic_hits = 1;
    recall_hurting.version = 3;
    let hurting_train = eval::evaluate_policy(train.clone(), &recall_hurting).await?;
    let hurting_holdout = eval::evaluate_policy(holdout.clone(), &recall_hurting).await?;
    let rejected = evaluate_promotion(
        &default_train,
        &hurting_train,
        &default_holdout,
        &hurting_holdout,
        &PromotionOptions::default(),
    );
    assert!(!rejected.promoted, "recall regresyonu reddedilmeli");

    // 5. Optimizer sonucu (sınırlı adayla) tutarlıdır: aynı seed aynı winner.
    let outcome_a = optimize_for_test(&tasks_file, out.path()).await?;
    let outcome_b = optimize_for_test(&tasks_file, out.path()).await?;
    assert_eq!(
        outcome_a.winner.version, outcome_b.winner.version,
        "aynı seed aynı winner üretmeli"
    );

    Ok(())
}

fn split_tasks(file: &GoldenTasksFile, split: &str) -> GoldenTasksFile {
    GoldenTasksFile {
        schema_version: file.schema_version,
        tasks: file
            .tasks
            .iter()
            .filter(|task| task.split.as_deref() == Some(split))
            .cloned()
            .collect(),
    }
}

async fn optimize_for_test(
    tasks_file: &GoldenTasksFile,
    _out: &std::path::Path,
) -> Result<OptimizationOutcome> {
    let train = split_tasks(tasks_file, "train");
    ccm_core::optimize::optimize(&train, FIXTURE_SEED, 6).await
}
