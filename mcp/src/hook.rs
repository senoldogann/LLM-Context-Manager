//! İsteğe bağlı Claude Code PreToolUse hook'u: tek bir sembolü arayan Grep'i,
//! indeks o sorunun tam cevabını grep çıktısından daha kısa verebiliyorsa,
//! `context` paketiyle karşılar.
//!
//! Ölçümler Claude Code'un CCM'i kendiliğinden hiç seçmediğini gösterdi (0/25
//! doğal koşu). Hook kurulumla gelen bir ürün yapılandırmasıdır, doğal kullanım
//! değildir. Karar kuralları bilerek dardır; aşağıdakilerden biri tutmazsa grep
//! olduğu gibi çalışır:
//! - Grep'te `-i`, `glob`, `type` yoktur; `path` boş ya da bir dizindir (tek dosya değil);
//! - desen, `\b`/`\(`/`\s*\(` sarmalayıcıları soyulunca tek bir ad ya da
//!   `alıcı.üye` ifadesidir ve bir tanım araması (`def ad`, `fn ad`) değildir;
//! - indeks tazedir (bekleyen değişiklik yoktur);
//! - paket "call list complete" der;
//! - paket, Grep'in aynı kapsamda döndüreceği tahmini çıktıdan kısadır;
//! - aynı oturumda aynı sembol için daha önce reddedilmemiştir.
//!
//! Hata ya da süre aşımı da grep'i geçirir; hook asla engelleyici bir hata üretmez.
//! Her karar nedeniyle `CCM_HOOK_LOG` dosyasına (varsa) JSON satırı olarak yazılır.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use serde_json::{json, Value};

/// Hook'un kendi süre sınırı; Claude Code ayarındaki zaman aşımından kısa olmalı.
const HOOK_BUDGET: Duration = Duration::from_secs(8);
/// Paket bütçesi (token); `context` aracının varsayılanıyla aynı.
const PACKET_TOKENS: u64 = 1_500;

/// Grep çağrısından okunan, karar için gereken alanlar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GrepCall {
    pub session_id: String,
    pub cwd: PathBuf,
    pub pattern: String,
    pub path: Option<String>,
    pub output_mode: String,
    pub case_insensitive: bool,
    pub glob: Option<String>,
    pub file_type: Option<String>,
}

/// Hook'un kararı: grep geçer (nedeniyle) ya da paketle reddedilir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    Pass(String),
    Deny { symbol: String, packet: String },
}

/// Stdin JSON'undan Grep çağrısı; Grep dışındaki araçlar `None`.
pub(crate) fn parse_grep_call(input: &Value) -> Option<GrepCall> {
    if input.get("tool_name").and_then(Value::as_str) != Some("Grep") {
        return None;
    }
    let tool = input.get("tool_input")?;
    let text = |key: &str| tool.get(key).and_then(Value::as_str).map(str::to_string);
    Some(GrepCall {
        session_id: input.get("session_id")?.as_str()?.to_string(),
        cwd: PathBuf::from(input.get("cwd")?.as_str()?),
        pattern: text("pattern")?,
        path: text("path"),
        output_mode: text("output_mode").unwrap_or_else(|| "files_with_matches".to_string()),
        case_insensitive: tool.get("-i").and_then(Value::as_bool) == Some(true),
        glob: text("glob"),
        file_type: text("type"),
    })
}

/// Desenden aranan sembol: sarmalayıcılar soyulur, tek ad ya da `alıcı.üye`
/// kalmalıdır. Kabul edilmezse nedeni döner.
pub(crate) fn grep_symbol(call: &GrepCall) -> std::result::Result<String, String> {
    if call.case_insensitive || call.glob.is_some() || call.file_type.is_some() {
        return Err("grep uses -i, glob or type".to_string());
    }
    let mut pattern = call.pattern.trim().to_string();
    for wrapper in ["\\b", "\\s*\\(", "\\(", "(", "\\s*"] {
        pattern = pattern.trim_start_matches(wrapper).to_string();
        pattern = pattern.trim_end_matches(wrapper).to_string();
    }
    let pattern = pattern.replace("\\.", ".");
    for keyword in ["def ", "class ", "fn ", "function ", "struct "] {
        if pattern.starts_with(keyword) {
            return Err("definition search".to_string());
        }
    }
    let identifier = |part: &str| {
        let mut chars = part.chars();
        chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_' || first == '$')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    };
    let parts: Vec<&str> = pattern.split('.').collect();
    if parts.is_empty() || parts.len() > 2 || !parts.iter().all(|part| identifier(part)) {
        return Err("pattern is not a single symbol".to_string());
    }
    Ok(pattern)
}

/// Grep'in döndüreceği çıktının karakter alt sınırı (Claude'un Grep biçimleri:
/// `content` satırlar, `count` dosya başına sayı, `files_with_matches` yollar).
/// Satır başına yalnız yol, kırpılmış metin ve iki ayraç sayılır: kayıtlı dökümlerde
/// gerçek çıktı bu sınırın üstünde kaldı; tahmin aşarsa hook grep'ten uzun cevap verirdi.
pub(crate) fn estimated_grep_chars(output_mode: &str, matches: &[(String, Vec<String>)]) -> usize {
    match output_mode {
        "content" => matches
            .iter()
            .map(|(path, lines)| {
                lines
                    .iter()
                    .map(|line| path.len() + line.trim().len() + 2)
                    .sum::<usize>()
            })
            .sum(),
        "count" => matches.iter().map(|(path, _)| path.len() + 2).sum(),
        _ => matches.iter().map(|(path, _)| path.len() + 1).sum(),
    }
}

/// Paketin tamlık başlığı "complete" mi.
pub(crate) fn packet_is_complete(packet: &str) -> bool {
    packet
        .lines()
        .any(|line| line.starts_with("Check: call list complete."))
}

/// Hook'un verdiği kısa paket: hedef satırı, tamlık başlığı ve çağıran başına tek
/// satır (`yol:Kapsam (çağrı satırları)`). Gövde, callee, üye ve test listeleri
/// atılır; "kim çağırıyor" cevabı ve tamlık kanıtı kalır. Tam pakette bu bölümler
/// bulunamazsa nedeni döner (hook grep'i geçirir).
pub(crate) fn compact_packet(packet: &str) -> std::result::Result<String, String> {
    let lines: Vec<&str> = packet.lines().collect();
    let target = lines
        .iter()
        .find(|line| line.contains("` · ") && !line.starts_with('-'))
        .ok_or_else(|| "packet has no target line".to_string())?;
    let check = lines
        .iter()
        .find(|line| line.starts_with("Check: call list complete."))
        .ok_or_else(|| "packet does not claim a complete call list".to_string())?;
    let start = lines
        .iter()
        .position(|line| *line == "callers:")
        .ok_or_else(|| "packet has no callers section".to_string())?;
    let mut output = vec![
        (*target).to_string(),
        (*check).to_string(),
        "callers:".to_string(),
    ];
    let mut current: Option<(String, Vec<String>)> = None;
    let flush = |current: &mut Option<(String, Vec<String>)>, output: &mut Vec<String>| {
        if let Some((caller, sites)) = current.take() {
            output.push(if sites.is_empty() {
                format!("- {caller}")
            } else {
                format!("- {caller} ({})", sites.join(", "))
            });
        }
    };
    for line in &lines[start + 1..] {
        if let Some(rest) = line.strip_prefix("- ") {
            flush(&mut current, &mut output);
            current = Some((caller_label(rest)?, Vec::new()));
        } else if let Some(rest) = line.strip_prefix("  + ") {
            match current.as_mut() {
                Some((_, sites)) if rest.ends_with(" more") => {
                    sites.push(format!("+{}", rest.trim_end_matches(" more")))
                }
                _ => {
                    flush(&mut current, &mut output);
                    output.push(format!("+ {rest}"));
                }
            }
        } else if let Some(more) = line
            .strip_prefix("  +")
            .and_then(|rest| rest.strip_suffix(" more"))
            .filter(|count| count.chars().all(|c| c.is_ascii_digit()))
        {
            // `  +2 more`: gösterilmeyen çağrı satırları (bkz. `  + N files` satırı).
            match current.as_mut() {
                Some((_, sites)) => sites.push(format!("+{more}")),
                None => return Err(format!("unexpected caller line {line:?}")),
            }
        } else if let Some(rest) = line.strip_prefix("  ") {
            let Some((number, _)) = rest.split_once(':') else {
                return Err(format!("unexpected caller line {line:?}"));
            };
            match current.as_mut() {
                Some((_, sites)) if number.chars().all(|c| c.is_ascii_digit()) => {
                    sites.push(number.to_string())
                }
                _ => return Err(format!("unexpected caller line {line:?}")),
            }
        } else {
            break;
        }
    }
    flush(&mut current, &mut output);
    Ok(output.join("\n"))
}

/// `Function: ad · yol:a-b · ilişki; scope: K; …` satırından `yol:K` (kapsam yoksa ad).
fn caller_label(line: &str) -> std::result::Result<String, String> {
    let parts: Vec<&str> = line.split(" · ").collect();
    let [kind_name, location, details, ..] = parts.as_slice() else {
        return Err(format!("unexpected caller line {line:?}"));
    };
    let name = kind_name
        .split_once(": ")
        .map_or(*kind_name, |(_, name)| name);
    let path = location
        .rsplit_once(':')
        .map_or(*location, |(path, _)| path);
    let scope = details
        .split("; ")
        .find_map(|part| part.strip_prefix("scope: "))
        .unwrap_or(name);
    Ok(format!("{path}:{scope}"))
}

/// Reddin modele gösterilen metni: hata değil, cevaptır; ham çıktıya yol açıktır.
pub(crate) fn deny_reason(symbol: &str, packet: &str) -> String {
    format!(
        "CCM answered this search for `{symbol}` from its code index (grep-equivalent check: call list complete). Run the same Grep again if you still need the raw matches.\n\n{packet}"
    )
}

/// Oturumun daha önce reddedilmiş sembollerinin dosyası.
fn session_state_path(session_id: &str) -> PathBuf {
    let safe: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    std::env::temp_dir().join(format!("ccm-hook-{safe}.json"))
}

fn denied_symbols(session_id: &str) -> Result<BTreeSet<String>> {
    let path = session_state_path(session_id);
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("hook state {} could not be read", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("hook state {} is not valid JSON", path.display()))
}

fn remember_denied(session_id: &str, symbol: &str) -> Result<()> {
    let mut symbols = denied_symbols(session_id)?;
    symbols.insert(symbol.to_string());
    let path = session_state_path(session_id);
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, serde_json::to_string(&symbols)?)?;
    std::fs::rename(&temporary, &path)?;
    Ok(())
}

/// Grep kapsamındaki indekslenmiş dosyalarda sembolün geçtiği satırlar.
async fn grep_matches(
    engine: &Arc<ccm_core::engine::RetrievalEngine>,
    root: &Path,
    scope: &str,
    symbol: &str,
) -> Result<Vec<(String, Vec<String>)>> {
    let files: Vec<String> = {
        let graph = engine.graph.read().await;
        graph
            .graph
            .node_weights()
            .filter(|node| node.node_type == ccm_core::graph::NodeType::File)
            .map(|node| node.id.trim_start_matches("./").to_string())
            .filter(|path| scope.is_empty() || path.starts_with(scope))
            .collect()
    };
    let mut matches = Vec::new();
    for file in files {
        let full = root.join(&file);
        let bytes = std::fs::read(&full)
            .with_context(|| format!("hook could not read {}", full.display()))?;
        let text = String::from_utf8(bytes)
            .with_context(|| format!("hook: {} is not UTF-8", full.display()))?;
        let lines: Vec<String> = text
            .lines()
            // Tam sembol, sözcük sınırlarıyla: tahmin Grep'in gerçek çıktısını hiçbir
            // zaman aşmaz (`res\.send` için ilgisiz `send` satırları sayılmaz), böylece
            // hook grep'i olduğundan pahalı sanıp engellemez.
            .filter(|line| !crate::call_sites::needle_positions(line, symbol).is_empty())
            .map(str::to_string)
            .collect();
        if !lines.is_empty() {
            matches.push((file, lines));
        }
    }
    Ok(matches)
}

/// Grep kapsamı: kökten göreli dizin önekı; tek dosya hedefi kabul edilmez.
fn grep_scope(call: &GrepCall) -> std::result::Result<String, String> {
    let Some(path) = call.path.as_deref() else {
        return Ok(String::new());
    };
    let full = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        call.cwd.join(path)
    };
    if full.is_file() {
        return Err("grep targets a single file".to_string());
    }
    let relative = full
        .strip_prefix(&call.cwd)
        .map_err(|_| "grep path is outside the project".to_string())?;
    let scope = relative.to_string_lossy().trim_end_matches('/').to_string();
    Ok(if scope.is_empty() || scope == "." {
        String::new()
    } else {
        format!("{scope}/")
    })
}

/// Kararın kendisi: indeks yüklenir, tazelik denetlenir, paket alınır ve
/// boyutlar karşılaştırılır.
async fn decide(call: &GrepCall) -> Result<Decision> {
    let symbol = match grep_symbol(call) {
        Ok(symbol) => symbol,
        Err(reason) => return Ok(Decision::Pass(reason)),
    };
    let scope = match grep_scope(call) {
        Ok(scope) => scope,
        Err(reason) => return Ok(Decision::Pass(reason)),
    };
    if denied_symbols(&call.session_id)?.contains(&symbol) {
        return Ok(Decision::Pass(
            "already answered once in this session".to_string(),
        ));
    }
    let state = crate::server::ServerState::new().await?;
    let root = call.cwd.to_string_lossy().to_string();
    let loaded = state.get_engine(Some(&root)).await?;
    if let crate::server::EngineSource::Live(live) = &loaded.source {
        if live.has_pending_changes().await? {
            return Ok(Decision::Pass("index has pending changes".to_string()));
        }
    }
    let result = crate::tools::context(
        &loaded.engine,
        &json!({ "query": symbol, "max_tokens": PACKET_TOKENS }),
        Some(&call.cwd),
    )
    .await?;
    let packet: String = result
        .content
        .iter()
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if result.is_error == Some(true) || !packet_is_complete(&packet) {
        return Ok(Decision::Pass(
            "packet does not claim a complete call list".to_string(),
        ));
    }
    let compact = match compact_packet(&packet) {
        Ok(compact) => compact,
        Err(reason) => return Ok(Decision::Pass(reason)),
    };
    let matches = grep_matches(&loaded.engine, &call.cwd, &scope, &symbol).await?;
    // Karşılaştırma modele gidecek metnin tamamıyla ve Grep çıktısının alt sınırıyla:
    // hook yalnız gerçekten daha az metin verdiğinde cevaplar.
    let grep_chars = estimated_grep_chars(&call.output_mode, &matches);
    let reason_chars = deny_reason(&symbol, &compact).chars().count();
    if reason_chars >= grep_chars {
        return Ok(Decision::Pass(format!(
            "answer ({reason_chars} chars) is not shorter than the grep output (at least {grep_chars} chars)"
        )));
    }
    remember_denied(&call.session_id, &symbol)?;
    Ok(Decision::Deny {
        symbol,
        packet: compact,
    })
}

/// Karar satırını `CCM_HOOK_LOG` dosyasına ekler (ayarlıysa).
fn log_decision(
    call: Option<&GrepCall>,
    decision: &str,
    detail: &str,
    elapsed: Duration,
) -> Result<()> {
    let Ok(path) = std::env::var("CCM_HOOK_LOG") else {
        return Ok(());
    };
    let line = json!({
        "decision": decision,
        "detail": detail,
        "pattern": call.map(|call| call.pattern.clone()),
        "path": call.and_then(|call| call.path.clone()),
        "output_mode": call.map(|call| call.output_mode.clone()),
        "elapsed_ms": elapsed.as_millis() as u64,
    });
    use std::io::Write as _;
    // Tek yazım: paralel hook'ların O_APPEND kayıtları satır ortasında karışmaz.
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("hook log {path} could not be opened"))?;
    file.write_all(format!("{line}\n").as_bytes())?;
    Ok(())
}

/// `ccm-mcp hook claude-pre-tool-use`: stdin'den Claude Code hook girdisini okur;
/// reddederse stdout'a PreToolUse kararını yazar, geçirirse hiçbir şey yazmaz.
pub(crate) async fn run_claude_pre_tool_use() -> Result<()> {
    let started = std::time::Instant::now();
    let mut raw = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw)
        .context("hook input could not be read from stdin")?;
    let input: Value = serde_json::from_str(&raw).context("hook input is not valid JSON")?;
    let Some(call) = parse_grep_call(&input) else {
        return log_decision(None, "pass", "not a Grep call", started.elapsed());
    };
    let decision = match tokio::time::timeout(HOOK_BUDGET, decide(&call)).await {
        Ok(Ok(decision)) => decision,
        Ok(Err(error)) => Decision::Pass(format!("error: {error:#}")),
        Err(_) => Decision::Pass("time budget exceeded".to_string()),
    };
    match decision {
        Decision::Pass(reason) => log_decision(Some(&call), "pass", &reason, started.elapsed()),
        Decision::Deny { symbol, packet } => {
            let output = json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": deny_reason(&symbol, &packet),
                }
            });
            println!("{output}");
            log_decision(
                Some(&call),
                "deny",
                &format!("{} packet chars", packet.len()),
                started.elapsed(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(pattern: &str) -> GrepCall {
        GrepCall {
            session_id: "s".to_string(),
            cwd: PathBuf::from("/repo"),
            pattern: pattern.to_string(),
            path: None,
            output_mode: "content".to_string(),
            case_insensitive: false,
            glob: None,
            file_type: None,
        }
    }

    #[test]
    fn symbols_are_extracted_only_from_plain_identifier_patterns() {
        assert_eq!(
            grep_symbol(&call("ensure_sync")),
            Ok("ensure_sync".to_string())
        );
        assert_eq!(
            grep_symbol(&call("\\bensure_sync\\s*\\(")),
            Ok("ensure_sync".to_string())
        );
        assert_eq!(
            grep_symbol(&call("res\\.send\\(")),
            Ok("res.send".to_string())
        );
        assert!(grep_symbol(&call("def ensure_sync")).is_err());
        assert!(grep_symbol(&call("escape|html")).is_err());
        assert!(grep_symbol(&call("a.b.c")).is_err());
        let mut insensitive = call("escape");
        insensitive.case_insensitive = true;
        assert!(grep_symbol(&insensitive).is_err());
    }

    #[test]
    fn grep_output_estimate_follows_the_output_mode() {
        let matches = vec![(
            "a.py".to_string(),
            vec!["x = f(1)".to_string(), "f(2)".to_string()],
        )];
        assert_eq!(
            estimated_grep_chars("content", &matches),
            (4 + 8 + 2) + (4 + 4 + 2)
        );
        assert_eq!(estimated_grep_chars("count", &matches), 6);
        assert_eq!(estimated_grep_chars("files_with_matches", &matches), 5);
    }

    #[test]
    fn compact_packet_keeps_the_check_and_one_line_per_caller() {
        let packet = "_Index: fresh_\n\nFunction `capfirst` · django/utils/text.py:25-31 · 2 callers\nCheck: call list complete. A grep finds 3 call sites.\n1 non-call use(s) outside:\n  a.py:1: import\ncallers:\n- Function: history_view · django/contrib/admin/options.py:2248-2306 · calls; scope: ModelAdmin.history_view\n  2288: x = capfirst(y)\n- Function: date_hierarchy · django/contrib/admin/templatetags/admin_list.py:361-462 · calls\n  405: a\n  408: b\n  +2 more\n  + 3 files that import it (module-level uses there count as inside)\n```\ndef capfirst(x):\n```\ncallees:\n- Function: f · a.py:1-2 · calls";
        let compact = compact_packet(packet).unwrap();
        assert_eq!(
            compact,
            "Function `capfirst` · django/utils/text.py:25-31 · 2 callers\nCheck: call list complete. A grep finds 3 call sites.\ncallers:\n- django/contrib/admin/options.py:ModelAdmin.history_view (2288)\n- django/contrib/admin/templatetags/admin_list.py:date_hierarchy (405, 408, +2)\n+ 3 files that import it (module-level uses there count as inside)"
        );
        assert!(compact_packet("Function `f` · a.py:1-2\nCheck: NOT complete.\ncallers:").is_err());
    }

    #[test]
    fn non_grep_tools_are_ignored() {
        let input = json!({"tool_name": "Read", "session_id": "s", "cwd": "/r", "tool_input": {}});
        assert!(parse_grep_call(&input).is_none());
    }
}
