//! `eval` komutunun rapor yazımı ve kalite kapısı kararı.
//!
//! Sıra sabittir: önce taban çizgisi okunur, sonra karar saf bir değer olarak
//! hesaplanır, en son rapor geçici dosya + rename ile atomik yazılır. Rapor ile
//! taban çizgisi aynı dosyaysa komut değerlendirmeden önce reddedilir: başarısız
//! bir koşu kendi taban çizgisinin üzerine yazıp sonraki koşuda geriletmeyi
//! görünmez kılamaz.

use anyhow::Context;
use ccm_core::eval::{ComparisonReport, EvalReport};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Kalite kapısının sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateVerdict {
    Passed,
    Failed { reason: String },
}

/// Saf karar: raporu eşik ve (varsa) taban çizgisiyle karşılaştırır.
pub fn gate_verdict(
    report: &EvalReport,
    minimum_pass_rate: f64,
    baseline: Option<&EvalReport>,
    maximum_regression: f64,
) -> GateVerdict {
    match ccm_core::eval::enforce_quality_gate(
        report,
        minimum_pass_rate,
        baseline,
        maximum_regression,
    ) {
        Ok(()) => GateVerdict::Passed,
        Err(error) => GateVerdict::Failed {
            reason: format!("{error:#}"),
        },
    }
}

/// Kalite kapısı başarısız olduğunda dönen hata.
#[derive(Debug)]
pub struct QualityGateError {
    pub reason: String,
}

impl std::fmt::Display for QualityGateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for QualityGateError {}

/// Kararı süreç sonucuna çevirir.
pub fn enforce_verdict(verdict: GateVerdict) -> Result<(), QualityGateError> {
    match verdict {
        GateVerdict::Passed => Ok(()),
        GateVerdict::Failed { reason } => Err(QualityGateError { reason }),
    }
}

/// `--report` ile `--baseline` aynı dosyayı gösterdiğinde dönen hata.
#[derive(Debug)]
pub struct SameReportAndBaselineError {
    pub report: PathBuf,
    pub baseline: PathBuf,
    pub resolved: PathBuf,
}

impl std::fmt::Display for SameReportAndBaselineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "--report '{}' and --baseline '{}' resolve to the same file '{}'; \
             write the report to a different file and copy it over the baseline after a passing run",
            self.report.display(),
            self.baseline.display(),
            self.resolved.display()
        )
    }
}

impl std::error::Error for SameReportAndBaselineError {}

/// Rapor ile taban çizgisinin farklı dosyalar olduğunu doğrular.
pub fn ensure_distinct_report_and_baseline(report: &Path, baseline: &Path) -> anyhow::Result<()> {
    let baseline_target = std::fs::canonicalize(baseline).with_context(|| {
        format!(
            "baseline report '{}' could not be resolved",
            baseline.display()
        )
    })?;
    let report_target = resolve_write_target(report)?;
    if report_target == baseline_target {
        return Err(SameReportAndBaselineError {
            report: report.to_path_buf(),
            baseline: baseline.to_path_buf(),
            resolved: report_target,
        }
        .into());
    }
    Ok(())
}

/// Yazılacak dosyanın kanonik yolu; dosya henüz yoksa kanonik üst dizin + ad.
fn resolve_write_target(path: &Path) -> anyhow::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(resolved) => Ok(resolved),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let file_name = path
                .file_name()
                .with_context(|| format!("report path '{}' has no file name", path.display()))?;
            let parent = parent_dir(path);
            let resolved_parent = std::fs::canonicalize(parent).with_context(|| {
                format!(
                    "directory '{}' of report '{}' could not be resolved",
                    parent.display(),
                    path.display()
                )
            })?;
            Ok(resolved_parent.join(file_name))
        }
        Err(error) => Err(error)
            .with_context(|| format!("report path '{}' could not be resolved", path.display())),
    }
}

/// Yolun üst dizini; yalın bir dosya adı için çalışma dizini.
fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Dosyayı aynı dizindeki geçici dosyaya yazar, diske indirir ve hedefe taşır.
/// Okuyucular yarım yazılmış bir rapor görmez; hata olursa hedef değişmez.
pub fn write_atomically(
    path: &Path,
    write: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let file_name = path
        .file_name()
        .with_context(|| format!("report path '{}' has no file name", path.display()))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_nanos();
    let temp_path = parent_dir(path).join(format!(
        ".{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        nanos
    ));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .with_context(|| {
            format!(
                "could not create temporary report '{}'",
                temp_path.display()
            )
        })?;
    let result = write_and_persist(file, write, &temp_path, path);
    if result.is_err() {
        if let Err(cleanup) = std::fs::remove_file(&temp_path) {
            tracing::warn!(
                path = %temp_path.display(),
                error = %cleanup,
                "Could not remove temporary report after a failed write"
            );
        }
    }
    result
}

/// Geçici dosyayı doldurur, `fsync` eder ve hedefin üzerine taşır.
fn write_and_persist(
    file: std::fs::File,
    write: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> anyhow::Result<()>,
    temp_path: &Path,
    path: &Path,
) -> anyhow::Result<()> {
    let mut writer = std::io::BufWriter::new(file);
    write(&mut writer)
        .with_context(|| format!("could not write report to '{}'", temp_path.display()))?;
    writer
        .flush()
        .with_context(|| format!("could not flush report '{}'", temp_path.display()))?;
    let file = writer
        .into_inner()
        .map_err(|error| error.into_error())
        .with_context(|| format!("could not flush report '{}'", temp_path.display()))?;
    file.sync_all()
        .with_context(|| format!("could not sync report '{}'", temp_path.display()))?;
    std::fs::rename(temp_path, path).with_context(|| {
        format!(
            "could not move report '{}' to '{}'",
            temp_path.display(),
            path.display()
        )
    })
}

/// Değerlendirme raporunu dosyaya (atomik) ya da stdout'a yazar.
pub fn emit_report(report: &EvalReport, path: Option<&Path>) -> anyhow::Result<()> {
    match path {
        Some(path) => {
            write_atomically(path, |writer| ccm_core::eval::write_report(writer, report))?;
            println!("Report written to {}", path.display());
            println!(
                "{}",
                ccm_core::eval::summarize_report(report, Some(&path.to_string_lossy()))
            );
        }
        None => {
            ccm_core::eval::write_report(std::io::stdout().lock(), report)?;
            println!();
        }
    }
    Ok(())
}

/// Karşılaştırma raporunu dosyaya (atomik) ya da stdout'a yazar.
pub fn emit_comparison_report(
    report: &ComparisonReport,
    path: Option<&Path>,
) -> anyhow::Result<()> {
    match path {
        Some(path) => {
            write_atomically(path, |writer| {
                ccm_core::eval::write_comparison_report(writer, report)
            })?;
            println!("Report written to {}", path.display());
            println!(
                "{}",
                ccm_core::eval::summarize_comparison_report(report, Some(&path.to_string_lossy()))
            );
        }
        None => {
            ccm_core::eval::write_comparison_report(std::io::stdout().lock(), report)?;
            println!();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ccm_core::eval::Totals;

    fn report(scored: usize, passed: usize) -> EvalReport {
        EvalReport {
            schema_version: 1,
            generated_at: 0,
            totals: Totals {
                tasks: scored,
                scored,
                passed,
                failed: scored - passed,
                skipped: 0,
            },
            results: Vec::new(),
        }
    }

    #[test]
    fn verdict_passes_without_regression() {
        let current = report(10, 9);
        let baseline = report(10, 9);
        assert_eq!(
            gate_verdict(&current, 50.0, Some(&baseline), 0.0),
            GateVerdict::Passed
        );
    }

    #[test]
    fn verdict_fails_on_regression_against_baseline() {
        let current = report(10, 7);
        let baseline = report(10, 9);
        let verdict = gate_verdict(&current, 0.0, Some(&baseline), 5.0);
        assert!(
            matches!(&verdict, GateVerdict::Failed { reason } if reason.contains("regression")),
            "{verdict:?}"
        );
    }

    #[test]
    fn verdict_fails_below_minimum_pass_rate() {
        let verdict = gate_verdict(&report(10, 4), 50.0, None, 0.0);
        assert!(
            matches!(&verdict, GateVerdict::Failed { reason } if reason.contains("below")),
            "{verdict:?}"
        );
    }
}
