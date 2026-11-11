//! İsteğe bağlı Claude Code PreToolUse hook'u: tek bir sembolü arayan Grep'i,
//! indeks o sorunun tam cevabını grep çıktısından daha kısa verebiliyorsa,
//! `context` paketiyle karşılar.
//!
//! Ölçümler Claude Code'un CCM'i kendiliğinden hiç seçmediğini gösterdi (0/25
//! doğal koşu). Hook kurulumla gelen bir ürün yapılandırmasıdır, doğal kullanım
//! değildir. Karar kuralları bilerek dardır; aşağıdakilerden biri tutmazsa grep
//! olduğu gibi çalışır:
//! - Grep'te `-i` ve `type` yoktur; `glob` ya yoktur ya da yalnız test, bench ve örnek
//!   dizinlerini dışlar (`!**/tests/**`, `!**/{tests,benches,examples}/**`): paketin
//!   kapsamı da test dışı dosyalardır. `path` boş ya da bir dizindir (tek dosya değil);
//! - desen, `\b`/`\(`/`\s*\(` sarmalayıcıları soyulunca tek bir ad ya da
//!   `alıcı.üye` ifadesidir ve bir tanım araması (`def ad`, `fn ad`) değildir;
//! - indeks tazedir (bekleyen değişiklik yoktur);
//! - paket "call list complete" der;
//! - paket, Grep'in aynı kapsamda döndüreceği tahmini çıktıdan kısadır.
//!
//! Aynı hook, Bash'teki tek bir sembol aramasını da (`grep -r…`/`rg …`, isteğe bağlı baştaki
//! `cd` ve sondaki `grep -v`/`head` süzgeçleriyle; bkz. `bash_search`) aynı kurallarla karşılar.
//! Bash eşleştiricisi yalnız E kolunda kuruludur.
//!
//! Aynı oturumda aynı sembol için ikinci Grep, paketteki çağıranların satır
//! aralıklarına düşen ham eşleşmelerle (`yol:satır: metin`) karşılanır: ajan ham
//! satırları ister, yeni bir arama turu gerekmez. Bu cevap da Grep'ten kısa değilse
//! ya da üçüncü kez sorulursa grep olduğu gibi çalışır.
//!
//! Hata ya da süre aşımı da grep'i geçirir; hook asla engelleyici bir hata üretmez.
//! Her karar nedeniyle `CCM_HOOK_LOG` dosyasına (varsa) JSON satırı olarak yazılır.

use std::collections::BTreeMap;
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
    /// Aranan dizinler, proje köküne (`cwd`) göre; boşsa bütün proje.
    pub paths: Vec<String>,
    /// Bash'teki `| grep -v METİN` süzgeçlerinin sabit metinleri: `yol:satır:metin` çıktı
    /// satırında geçenler Grep çıktısında yoktur. Grep aracında boştur.
    pub dropped: Vec<String>,
    pub output_mode: String,
    pub case_insensitive: bool,
    pub glob: Option<String>,
    pub file_type: Option<String>,
}

/// Hook'un kararı: grep geçer (nedeniyle) ya da modele gidecek cevap metniyle reddedilir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    Pass(String),
    Deny { answer: String },
}

/// Oturumda bir sembol için verilmiş cevap: önce paket, sonra ham çağrı satırları.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Answered {
    Packet,
    CallSiteLines,
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
        paths: text("path").into_iter().collect(),
        dropped: Vec::new(),
        output_mode: text("output_mode").unwrap_or_else(|| "files_with_matches".to_string()),
        case_insensitive: tool.get("-i").and_then(Value::as_bool) == Some(true),
        glob: text("glob"),
        file_type: text("type"),
    })
}

/// Bash komutunun sözcükleri: tırnaklar açılır (çift tırnak içinde ters bölü korunur: düzenli
/// ifade kaçışları desenin parçasıdır), tırnak dışındaki `|`, `;` ve `&&` ayrı sözcüktür.
/// Desteklenmeyen kabuk sözdiziminde (`$(`, ters tırnak, yönlendirme, kapanmayan tırnak) hata.
pub(crate) fn shell_words(command: &str) -> std::result::Result<Vec<String>, String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = command.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(inner) => current.push(inner),
                        None => return Err("unclosed quote".to_string()),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('$') | Some('`') => {
                            return Err("command substitution or variable".to_string())
                        }
                        Some('\\') => match chars.next() {
                            Some(escaped @ ('"' | '\\')) => current.push(escaped),
                            Some(other) => {
                                current.push('\\');
                                current.push(other);
                            }
                            None => return Err("unclosed quote".to_string()),
                        },
                        Some(inner) => current.push(inner),
                        None => return Err("unclosed quote".to_string()),
                    }
                }
            }
            '$' | '`' | '>' | '<' | '(' | ')' | '\n' => {
                return Err(format!("unsupported shell syntax {character:?}"))
            }
            '|' | ';' | '&' => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
                if character == '&' {
                    if chars.next() != Some('&') {
                        return Err("background job".to_string());
                    }
                    words.push("&&".to_string());
                } else if character == '|' && chars.peek() == Some(&'|') {
                    return Err("|| list".to_string());
                } else {
                    words.push(character.to_string());
                }
            }
            whitespace if whitespace.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\\' => {
                started = true;
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            other => {
                started = true;
                current.push(other);
            }
        }
    }
    if started {
        words.push(current);
    }
    Ok(words)
}

/// Bash komutundaki tek arama: isteğe bağlı baştaki `cd DİZİN &&`/`;`, sonra `grep -r…` ya da
/// `rg …`, ardından yalnız `grep -v…` ve `head` filtreleri. Çıktıyı değiştiren bayraklar
/// (`-i`, `-l`, `-c`, `-o`, bağlam satırları), özyinelemesiz `grep` ve birden çok komut
/// desteklenmez. Yollar proje köküne göre yazılır; `cd` dizini önlerine eklenir.
pub(crate) fn bash_search(
    session_id: &str,
    cwd: &Path,
    command: &str,
) -> std::result::Result<GrepCall, String> {
    let words = shell_words(command)?;
    let mut rest: &[String] = &words;
    let mut directory = String::new();
    if rest.first().map(String::as_str) == Some("cd") {
        let (Some(target), Some(separator)) = (rest.get(1), rest.get(2)) else {
            return Err("cd without a following command".to_string());
        };
        if separator != "&&" && separator != ";" {
            return Err("cd is not followed by a command".to_string());
        }
        directory = target.trim_end_matches('/').to_string();
        rest = &rest[3..];
    }
    if rest.iter().any(|word| word == ";" || word == "&&") {
        return Err("more than one command".to_string());
    }
    let segments: Vec<&[String]> = rest.split(|word| word == "|").collect();
    let (search, filters) = segments
        .split_first()
        .ok_or_else(|| "empty command".to_string())?;
    let mut dropped: Vec<String> = Vec::new();
    for filter in filters {
        dropped.push(fixed_inverse_filter(filter)?);
    }
    let program = search.first().map(String::as_str);
    let mut pattern: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut excluded: Vec<String> = Vec::new();
    let mut recursive = program == Some("rg");
    let mut words = search.iter().skip(1);
    while let Some(word) = words.next() {
        let flag = word.as_str();
        match (program, flag) {
            (_, "-e") => pattern = words.next().cloned(),
            (Some("rg"), "-g" | "--glob") => {
                let glob = words
                    .next()
                    .ok_or_else(|| "glob without value".to_string())?;
                excluded.extend(glob_exclusions(glob)?);
            }
            (Some("rg"), _) if flag.starts_with("--glob=") => {
                excluded.extend(glob_exclusions(&flag["--glob=".len()..])?);
            }
            (Some("rg"), "-t" | "--type") => {
                words.next();
            }
            (Some("rg"), "-n" | "-w" | "-F" | "--no-heading" | "-N" | "--line-number") => {}
            (Some("grep"), _) if flag.starts_with("--include=") => {}
            (Some("grep"), "--include") => {
                words.next();
            }
            (Some("grep"), _) if flag.starts_with("--exclude-dir=") => {
                let directory = &flag["--exclude-dir=".len()..];
                let names: Vec<&str> = directory
                    .trim_start_matches('{')
                    .trim_end_matches('}')
                    .split(',')
                    .collect();
                if !names
                    .iter()
                    .all(|name| NON_LIBRARY_DIRECTORIES.contains(name))
                {
                    return Err("exclude-dir other than tests, benches or examples".to_string());
                }
                excluded.extend(names.iter().map(|name| (*name).to_string()));
            }
            (Some("grep"), _)
                if flag.starts_with('-')
                    && !flag.starts_with("--")
                    && flag.len() > 1
                    && flag[1..].chars().all(|letter| "rRnEwHs".contains(letter)) =>
            {
                recursive |= flag.contains('r') || flag.contains('R');
            }
            (_, _) if flag.starts_with('-') => return Err(format!("unsupported flag {flag}")),
            (_, _) => positional.push(word.clone()),
        }
    }
    if !matches!(program, Some("grep") | Some("rg")) {
        return Err("not a grep or rg search".to_string());
    }
    if !recursive {
        return Err("grep without -r searches files or stdin".to_string());
    }
    let pattern = match pattern {
        Some(pattern) => pattern,
        None if !positional.is_empty() => positional.remove(0),
        None => return Err("no pattern".to_string()),
    };
    let base = |path: &str| -> String {
        let joined = if directory.is_empty() {
            path.to_string()
        } else if path == "." {
            directory.clone()
        } else {
            format!("{directory}/{path}")
        };
        joined.trim_start_matches("./").to_string()
    };
    let paths: Vec<String> = if positional.is_empty() {
        if directory.is_empty() {
            Vec::new()
        } else {
            vec![base(".")]
        }
    } else {
        positional.iter().map(|path| base(path)).collect()
    };
    excluded.sort();
    excluded.dedup();
    Ok(GrepCall {
        session_id: session_id.to_string(),
        cwd: cwd.to_path_buf(),
        pattern,
        paths,
        output_mode: "content".to_string(),
        case_insensitive: false,
        glob: (!excluded.is_empty()).then(|| format!("!**/{{{}}}/**", excluded.join(","))),
        file_type: None,
        dropped,
    })
}

/// `grep -v METİN` süzgecinin sabit metni. Düzenli ifade (özel karakterli, `-F` olmadan),
/// birden çok desen ve `head` gibi başka aşamalar desteklenmez: çıktı boyutu doğru tahmin
/// edilemez, hook komutu geçirir.
fn fixed_inverse_filter(stage: &[String]) -> std::result::Result<String, String> {
    if stage.first().map(String::as_str) != Some("grep") {
        return Err("a pipe stage other than grep -v".to_string());
    }
    let (flags, patterns): (Vec<&String>, Vec<&String>) =
        stage.iter().skip(1).partition(|word| word.starts_with('-'));
    let plain_flags = flags
        .iter()
        .all(|flag| flag[1..].chars().all(|letter| "vF".contains(letter)));
    if !plain_flags || !flags.iter().any(|flag| flag.contains('v')) {
        return Err("a pipe stage other than a plain grep -v".to_string());
    }
    let [pattern] = patterns.as_slice() else {
        return Err("grep -v with more than one pattern".to_string());
    };
    let special = pattern
        .chars()
        .any(|letter| "\\^$.*[]|()+?{}".contains(letter));
    let fixed = flags.iter().any(|flag| flag.contains('F'));
    if special && !fixed {
        return Err("grep -v with a regular expression".to_string());
    }
    Ok((*pattern).clone())
}

/// `rg -g` değerinin dışladığı dizinler: dosya türü süzgeci (`*.py`) hiçbirini dışlamaz.
fn glob_exclusions(glob: &str) -> std::result::Result<Vec<String>, String> {
    if !glob.starts_with('!') {
        return if glob.starts_with("*.") || glob.starts_with("**/*.") {
            Ok(Vec::new())
        } else {
            Err(format!("glob {glob} narrows the search"))
        };
    }
    excluded_directories(glob)
        .or_else(|| {
            glob.strip_prefix("!")
                .and_then(|inner| inner.strip_suffix("/**"))
                .map(|inner| inner.trim_start_matches("**/"))
                .filter(|name| NON_LIBRARY_DIRECTORIES.contains(name))
                .map(|name| vec![name.to_string()])
        })
        .ok_or_else(|| format!("glob {glob} is not a test, bench or example exclusion"))
}

/// Stdin JSON'undaki Bash aramasının çağrısı; Bash dışı araçlar `None`.
fn bash_command(input: &Value) -> Option<std::result::Result<GrepCall, String>> {
    if input.get("tool_name").and_then(Value::as_str) != Some("Bash") {
        return None;
    }
    let session = input.get("session_id")?.as_str()?;
    let cwd = PathBuf::from(input.get("cwd")?.as_str()?);
    let command = input.get("tool_input")?.get("command")?.as_str()?;
    Some(bash_search(session, &cwd, command))
}

/// Paketin kapsamı dışında kalan, glob'un dışlayabileceği dizinler.
const NON_LIBRARY_DIRECTORIES: [&str; 6] =
    ["tests", "test", "benches", "examples", "__tests__", "spec"];

/// Yalnız test, bench ya da örnek dizinlerini dışlayan glob'un dizin adları
/// (`!**/tests/**` → [`tests`], `!**/{tests,benches}/**` → [`tests`, `benches`]);
/// başka her glob `None`dır.
pub(crate) fn excluded_directories(glob: &str) -> Option<Vec<String>> {
    let inner = glob.strip_prefix("!**/")?.strip_suffix("/**")?;
    let names: Vec<&str> = match inner
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    {
        Some(list) => list.split(',').map(str::trim).collect(),
        None => vec![inner],
    };
    names
        .iter()
        .all(|name| NON_LIBRARY_DIRECTORIES.contains(name))
        .then(|| names.iter().map(|name| (*name).to_string()).collect())
}

/// Grep'in glob'unun dışladığı dizinler; glob yoksa boş, kabul edilmeyen glob'da hata.
fn grep_exclusions(call: &GrepCall) -> std::result::Result<Vec<String>, String> {
    match call.glob.as_deref() {
        None => Ok(Vec::new()),
        Some(glob) => excluded_directories(glob)
            .ok_or_else(|| "grep glob is not a test, bench or example exclusion".to_string()),
    }
}

/// Desenden aranan sembol: sarmalayıcılar soyulur, tek ad ya da `alıcı.üye`
/// kalmalıdır. Kabul edilmezse nedeni döner.
pub(crate) fn grep_symbol(call: &GrepCall) -> std::result::Result<String, String> {
    if call.case_insensitive || call.file_type.is_some() {
        return Err("grep uses -i or type".to_string());
    }
    grep_exclusions(call)?;
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

/// Yol, glob'un dışladığı dizinlerden birinde mi.
fn in_excluded_directory(path: &str, excluded: &[String]) -> bool {
    excluded.iter().any(|directory| {
        path.starts_with(&format!("{directory}/")) || path.contains(&format!("/{directory}/"))
    })
}

/// Kısa paketten glob'un dışladığı dizinlerdeki çağıranları atar (Grep onları da
/// göstermezdi) ve kaç tanesinin atıldığını tek satırla söyler.
pub(crate) fn without_excluded_callers(compact: &str, excluded: &[String]) -> String {
    let mut dropped = 0;
    let mut lines: Vec<&str> = Vec::new();
    for line in compact.lines() {
        let path = line
            .strip_prefix("- ")
            .and_then(|rest| rest.split_once(':'))
            .map(|(path, _)| path);
        match path {
            Some(path) if in_excluded_directory(path, excluded) => dropped += 1,
            _ => lines.push(line),
        }
    }
    let mut text = lines.join("\n");
    if dropped > 0 {
        text.push_str(&format!(
            "\n+ {dropped} callers in {} not listed (excluded by the Grep's glob)",
            excluded.join(", ")
        ));
    }
    text
}

/// `Function: ad · yol:a-b · ilişki; scope: K; …` satırından `yol:K` (kapsam yoksa ad).
/// Kısa paket ilişki etiketini taşımaz; bu yüzden yalnız kanıtlanmış `calls` ilişkisi
/// kabul edilir. `may call (receiver not resolved)` gibi bir satır (ör. thread-local
/// `CONTEXT.try_with(..)`'in görev-yerel `try_with`'e bağlanması) hatadır: hook grep'i geçirir.
fn caller_label(line: &str) -> std::result::Result<String, String> {
    let parts: Vec<&str> = line.split(" · ").collect();
    let [kind_name, location, details, ..] = parts.as_slice() else {
        return Err(format!("unexpected caller line {line:?}"));
    };
    let relation = details.split("; ").next().unwrap_or(details).trim();
    if relation != "calls" {
        return Err(format!("caller relation is not a proven call: {relation}"));
    }
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

/// İlk reddin modele gösterilen metni: hata değil, cevaptır.
pub(crate) fn deny_reason(symbol: &str, packet: &str) -> String {
    format!(
        "CCM answered this search for `{symbol}` from its code index (grep-equivalent check: call list complete): every call of `{symbol}` in the indexed non-test files is inside the callers below.\n\n{packet}"
    )
}

/// Paketteki çağıranların dosyası ve satır aralığı (`- Tür: ad · yol:a-b · …`).
pub(crate) fn caller_ranges(packet: &str) -> Vec<(String, usize, usize)> {
    let Some(start) = packet.lines().position(|line| line == "callers:") else {
        return Vec::new();
    };
    packet
        .lines()
        .skip(start + 1)
        .take_while(|line| line.starts_with("- ") || line.starts_with("  "))
        .filter_map(|line| line.strip_prefix("- "))
        .filter_map(|line| {
            let location = line.split(" · ").nth(1)?;
            let (path, range) = location.rsplit_once(':')?;
            let (first, last) = range.split_once('-')?;
            Some((path.to_string(), first.parse().ok()?, last.parse().ok()?))
        })
        .collect()
}

/// Eşleşmelerden çağıran aralıklarına düşenler, Grep'in `content` biçiminde
/// (`yol:satır: metin`).
pub(crate) fn call_site_lines(
    ranges: &[(String, usize, usize)],
    matches: &[(String, Vec<(usize, String)>)],
) -> Vec<String> {
    matches
        .iter()
        .flat_map(|(path, lines)| {
            lines
                .iter()
                .filter(move |(number, _)| {
                    ranges.iter().any(|(file, first, last)| {
                        file == path && (*first..=*last).contains(number)
                    })
                })
                .map(move |(number, text)| format!("{path}:{number}: {}", text.trim()))
        })
        .collect()
}

/// İkinci reddin metni: tekrarlanan Grep'e çağrı yerlerindeki ham satırlar.
pub(crate) fn repeat_reason(symbol: &str, lines: &[String], other: usize) -> String {
    format!(
        "CCM: this repeats the search for `{symbol}` answered above. These are its raw matches at the call sites listed there ({} lines); the other {other} matching lines in the indexed non-test files are imports, definitions, comments, strings or non-call uses.\n\n{}",
        lines.len(),
        lines.join("\n")
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

fn answered_symbols(session_id: &str) -> Result<BTreeMap<String, Answered>> {
    let path = session_state_path(session_id);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("hook state {} could not be read", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("hook state {} is not valid JSON", path.display()))
}

fn remember_answer(session_id: &str, symbol: &str, answered: Answered) -> Result<()> {
    let mut symbols = answered_symbols(session_id)?;
    symbols.insert(symbol.to_string(), answered);
    let path = session_state_path(session_id);
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, serde_json::to_string(&symbols)?)?;
    std::fs::rename(&temporary, &path)?;
    Ok(())
}

/// Grep kapsamındaki indekslenmiş dosyalarda sembolün geçtiği satırlar, numaralarıyla.
async fn grep_matches(
    engine: &Arc<ccm_core::engine::RetrievalEngine>,
    root: &Path,
    scope: &str,
    excluded: &[String],
    symbol: &str,
) -> Result<Vec<(String, Vec<(usize, String)>)>> {
    let files: Vec<String> = {
        let graph = engine.graph.read().await;
        graph
            .graph
            .node_weights()
            .filter(|node| node.node_type == ccm_core::graph::NodeType::File)
            .map(|node| node.id.trim_start_matches("./").to_string())
            .filter(|path| scope.is_empty() || path.starts_with(scope))
            // Glob'un dışladığı dizinlerdeki eşleşmeler Grep çıktısında yoktur.
            .filter(|path| !in_excluded_directory(path, excluded))
            .collect()
    };
    let mut matches = Vec::new();
    for file in files {
        let full = root.join(&file);
        let bytes = std::fs::read(&full)
            .with_context(|| format!("hook could not read {}", full.display()))?;
        let text = String::from_utf8(bytes)
            .with_context(|| format!("hook: {} is not UTF-8", full.display()))?;
        let lines: Vec<(usize, String)> = text
            .lines()
            .enumerate()
            // Tam sembol, sözcük sınırlarıyla: tahmin Grep'in gerçek çıktısını hiçbir
            // zaman aşmaz (`res\.send` için ilgisiz `send` satırları sayılmaz), böylece
            // hook grep'i olduğundan pahalı sanıp engellemez.
            .filter(|(_, line)| !crate::call_sites::needle_positions(line, symbol).is_empty())
            .map(|(index, line)| (index + 1, line.to_string()))
            .collect();
        if !lines.is_empty() {
            matches.push((file, lines));
        }
    }
    Ok(matches)
}

/// Grep kapsamları: kökten göreli dizin önekleri (boş önek bütün proje); tek dosya hedefi
/// kabul edilmez.
fn grep_scopes(call: &GrepCall) -> std::result::Result<Vec<String>, String> {
    if call.paths.is_empty() {
        return Ok(vec![String::new()]);
    }
    call.paths
        .iter()
        .map(|path| grep_scope(call, path))
        .collect()
}

fn grep_scope(call: &GrepCall, path: &str) -> std::result::Result<String, String> {
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
    let scopes = match grep_scopes(call) {
        Ok(scopes) => scopes,
        Err(reason) => return Ok(Decision::Pass(reason)),
    };
    let previous = answered_symbols(&call.session_id)?.get(&symbol).copied();
    if previous == Some(Answered::CallSiteLines) {
        return Ok(Decision::Pass(
            "already answered twice in this session".to_string(),
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
    let excluded = match grep_exclusions(call) {
        Ok(excluded) => excluded,
        Err(reason) => return Ok(Decision::Pass(reason)),
    };
    let compact = without_excluded_callers(&compact, &excluded);
    let mut numbered: Vec<(String, Vec<(usize, String)>)> = Vec::new();
    for scope in &scopes {
        for (path, lines) in
            grep_matches(&loaded.engine, &call.cwd, scope, &excluded, &symbol).await?
        {
            if !numbered.iter().any(|(seen, _)| *seen == path) {
                numbered.push((path, lines));
            }
        }
    }
    // Bash süzgeçlerinin çıkardığı satırlar Grep çıktısında yoktur.
    let numbered: Vec<(String, Vec<(usize, String)>)> = numbered
        .into_iter()
        .map(|(path, lines)| {
            let kept: Vec<(usize, String)> = lines
                .into_iter()
                .filter(|(number, text)| {
                    let shown = format!("{path}:{number}:{text}");
                    !call
                        .dropped
                        .iter()
                        .any(|dropped| shown.contains(dropped.as_str()))
                })
                .collect();
            (path, kept)
        })
        .filter(|(_, lines)| !lines.is_empty())
        .collect();
    let matches: Vec<(String, Vec<String>)> = numbered
        .iter()
        .map(|(path, lines)| {
            (
                path.clone(),
                lines.iter().map(|(_, text)| text.clone()).collect(),
            )
        })
        .collect();
    // Karşılaştırma modele gidecek metnin tamamıyla ve Grep çıktısının alt sınırıyla:
    // hook yalnız gerçekten daha az metin verdiğinde cevaplar.
    let grep_chars = estimated_grep_chars(&call.output_mode, &matches);
    let (answer, answered) = match previous {
        None => (deny_reason(&symbol, &compact), Answered::Packet),
        Some(_) => {
            let ranges: Vec<(String, usize, usize)> = caller_ranges(&packet)
                .into_iter()
                .filter(|(path, _, _)| !in_excluded_directory(path, &excluded))
                .collect();
            let lines = call_site_lines(&ranges, &numbered);
            let total: usize = numbered.iter().map(|(_, lines)| lines.len()).sum();
            (
                repeat_reason(&symbol, &lines, total.saturating_sub(lines.len())),
                Answered::CallSiteLines,
            )
        }
    };
    let answer_chars = answer.chars().count();
    if answer_chars >= grep_chars {
        return Ok(Decision::Pass(format!(
            "answer ({answer_chars} chars) is not shorter than the grep output (at least {grep_chars} chars)"
        )));
    }
    remember_answer(&call.session_id, &symbol, answered)?;
    Ok(Decision::Deny { answer })
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
        "path": call.map(|call| call.paths.join(" ")),
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
    let call = match parse_grep_call(&input) {
        Some(call) => call,
        None => match bash_command(&input) {
            None => return log_decision(None, "pass", "not a Grep call", started.elapsed()),
            Some(Err(reason)) => {
                return log_decision(None, "pass", &format!("bash: {reason}"), started.elapsed())
            }
            Some(Ok(call)) => call,
        },
    };
    let decision = match tokio::time::timeout(HOOK_BUDGET, decide(&call)).await {
        Ok(Ok(decision)) => decision,
        Ok(Err(error)) => Decision::Pass(format!("error: {error:#}")),
        Err(_) => Decision::Pass("time budget exceeded".to_string()),
    };
    match decision {
        Decision::Pass(reason) => log_decision(Some(&call), "pass", &reason, started.elapsed()),
        Decision::Deny { answer } => {
            let output = json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": answer,
                }
            });
            println!("{output}");
            log_decision(
                Some(&call),
                "deny",
                &format!("{} answer chars", answer.len()),
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
            paths: Vec::new(),
            dropped: Vec::new(),
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
    fn only_test_bench_and_example_exclusion_globs_are_accepted() {
        assert_eq!(
            excluded_directories("!**/tests/**"),
            Some(vec!["tests".to_string()])
        );
        assert_eq!(
            excluded_directories("!**/{tests,benches,examples}/**"),
            Some(vec![
                "tests".to_string(),
                "benches".to_string(),
                "examples".to_string()
            ])
        );
        assert_eq!(excluded_directories("*.py"), None);
        assert_eq!(excluded_directories("!**/{tests,src}/**"), None);
        assert_eq!(excluded_directories("{tokio,tokio-util}/src/**"), None);
        let mut scoped = call("enter_runtime");
        scoped.glob = Some("!**/{tests,benches,examples}/**".to_string());
        assert_eq!(grep_symbol(&scoped), Ok("enter_runtime".to_string()));
        scoped.glob = Some("*.py".to_string());
        assert!(grep_symbol(&scoped).is_err());
    }

    #[test]
    fn callers_in_excluded_directories_are_dropped_and_counted() {
        let compact = "Function `unbounded_channel` · tokio/src/sync/mpsc/unbounded.rs:1-2 · 3 callers\nCheck: call list complete.\ncallers:\n- tokio-test/src/io.rs:Inner.new (188)\n- benches/sync_mpsc.rs:contention_unbounded (40)\n- tokio/src/x/examples/demo.rs:run (3)";
        assert_eq!(
            without_excluded_callers(compact, &["benches".to_string(), "examples".to_string()]),
            "Function `unbounded_channel` · tokio/src/sync/mpsc/unbounded.rs:1-2 · 3 callers\nCheck: call list complete.\ncallers:\n- tokio-test/src/io.rs:Inner.new (188)\n+ 2 callers in benches, examples not listed (excluded by the Grep's glob)"
        );
        assert_eq!(without_excluded_callers(compact, &[]), compact);
    }

    #[test]
    fn repeated_grep_gets_only_the_lines_at_the_listed_call_sites() {
        let packet = "Function `get_text_list` · django/utils/text.py:290-310 · 2 callers\nCheck: call list complete.\ncallers:\n- Method: get_change_message · django/contrib/admin/models.py:150-200 · calls; scope: LogEntry.get_change_message\n  191: x = get_text_list(a)\n- Function: handle · django/core/management/commands/makemessages.py:380-390 · calls\n  386: get_text_list(b)\n```\ndef get_text_list(list_, last_word):\n```";
        let ranges = caller_ranges(packet);
        assert_eq!(
            ranges,
            vec![
                ("django/contrib/admin/models.py".to_string(), 150, 200),
                (
                    "django/core/management/commands/makemessages.py".to_string(),
                    380,
                    390
                ),
            ]
        );
        let matches = vec![
            (
                "django/contrib/admin/models.py".to_string(),
                vec![
                    (
                        12,
                        "from django.utils.text import get_text_list".to_string(),
                    ),
                    (191, "    x = get_text_list(a)".to_string()),
                ],
            ),
            (
                "django/utils/text.py".to_string(),
                vec![(290, "def get_text_list(list_, last_word):".to_string())],
            ),
        ];
        let lines = call_site_lines(&ranges, &matches);
        assert_eq!(
            lines,
            vec!["django/contrib/admin/models.py:191: x = get_text_list(a)"]
        );
        let reason = repeat_reason("get_text_list", &lines, 2);
        assert!(reason.contains("(1 lines)") && reason.contains("other 2 matching lines"));
        assert!(!deny_reason("get_text_list", "p").contains("Run the same Grep again"));
    }

    #[test]
    fn bash_searches_from_recorded_runs_are_parsed() {
        let cwd = Path::new("/repo");
        let call = bash_search("s", cwd, r#"grep -rn "get_current_timezone\b" django"#).unwrap();
        assert_eq!(call.pattern, r"get_current_timezone\b");
        assert_eq!(call.paths, vec!["django".to_string()]);
        assert!(call.dropped.is_empty());
        assert_eq!(grep_symbol(&call), Ok("get_current_timezone".to_string()));
        let call = bash_search(
            "s",
            cwd,
            r#"cd django && grep -rn "resolve_url" --include=*.py . | grep -v "/tests/""#,
        )
        .unwrap();
        assert_eq!(call.paths, vec!["django".to_string()]);
        assert_eq!(call.dropped, vec!["/tests/".to_string()]);
        let call = bash_search(
            "s",
            cwd,
            "rg -n '\\buninit\\(' tokio tokio-util -g '!**/tests/**' -g '!**/benches/**' | grep -v 'MaybeUninit::uninit'",
        )
        .unwrap();
        assert_eq!(call.pattern, r"\buninit\(");
        assert_eq!(
            call.paths,
            vec!["tokio".to_string(), "tokio-util".to_string()]
        );
        assert_eq!(call.glob.as_deref(), Some("!**/{benches,tests}/**"));
        assert_eq!(grep_symbol(&call), Ok("uninit".to_string()));
        assert_eq!(call.dropped, vec!["MaybeUninit::uninit".to_string()]);
        for unsupported in [
            "cd django; for f in a.py:1 b.py:2; do sed -n 1p $f; done",
            r#"grep -n "^class " utils/dateformat.py"#,
            r#"grep -rn "check_scoring" . | wc -l"#,
            r#"grep -rni "x" django"#,
            r#"rg -l "x" django"#,
            // Düzenli ifade süzgeci: çıktı boyutu tahmin edilemez.
            r#"grep -rn "x" django | grep -v "def x\b""#,
            r#"grep -rn "x" django | head -5"#,
        ] {
            assert!(bash_search("s", cwd, unsupported).is_err(), "{unsupported}");
        }
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
        let unresolved = "Function `try_with` · tokio/src/task/task_local.rs:245-264 · 2 callers\nCheck: call list complete. A grep finds 2 call sites.\ncallers:\n- Method: with · tokio/src/task/task_local.rs:230-240 · calls; scope: LocalKey.with\n  234: self.try_with(f)\n- Function: budget · tokio/src/runtime/context.rs:138-140 · may call (receiver not resolved)\n  139: CONTEXT.try_with(|ctx| f(&ctx.budget))";
        assert_eq!(
            compact_packet(unresolved),
            Err(
                "caller relation is not a proven call: may call (receiver not resolved)"
                    .to_string()
            )
        );
    }

    #[test]
    fn non_grep_tools_are_ignored() {
        let input = json!({"tool_name": "Read", "session_id": "s", "cwd": "/r", "tool_input": {}});
        assert!(parse_grep_call(&input).is_none());
    }
}
