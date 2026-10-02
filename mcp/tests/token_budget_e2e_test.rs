//! M2: token bütçeli, hedef kabul eden ve tek çağrıda cevap veren araçlar.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const CORE: &str = r#"from app.util import helper


class Engine:
    def start(self):
        self.stop()
        return helper()

    def stop(self):
        return 0


def run():
    # helper() yorumda: kenar değil
    text = "helper()"
    engine = Engine()
    return engine.start()
"#;

/// Python fikstürü: M1 fikstürü + çok çağıranlı `hub` + bir test dosyası.
fn fixture_files() -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = vec![
        ("app/__init__.py".into(), "from .core import run as start_app\n".into()),
        ("app/util.py".into(), "def helper():\n    return 1\n".into()),
        (
            "app/other.py".into(),
            "def helper():\n    return 2\n\n\ndef start():\n    return 3\n".into(),
        ),
        ("app/core.py".into(), CORE.into()),
        (
            "app/cli.py".into(),
            "from app.core import run\n\n\ndef main():\n    return run()\n".into(),
        ),
        (
            "app/models.py".into(),
            "class Base:\n    def save(self):\n        return 1\n\n\nclass User(Base):\n    def save(self):\n        return super().save()\n".into(),
        ),
        (
            "tests/test_core.py".into(),
            "from app.core import run\n\n\ndef test_run():\n    assert run() is not None\n".into(),
        ),
    ];
    // Çok baytlı gövde: bütçe karakterle ölçülmeli, bayt değil.
    files.push((
        "app/cjk.py".into(),
        "def cjk():\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    # 这是一个很长的中文注释，用来测试预算按字符计算。\n    return 1\n".into(),
    ));
    files.push((
        "app/cjk_users.py".into(),
        "from app.cjk import cjk\n\n\ndef use_1():\n    return cjk()\n\n\ndef use_2():\n    return cjk()\n\n\ndef use_3():\n    return cjk()\n".into(),
    ));
    files.push((
        "app/nested.py".into(),
        "class Outer:\n    class Inner:\n        def go(self):\n            return 1\n".into(),
    ));
    // Rust: `impl Foo` ayrı bir düğümdür; çıplak `Foo` yapının kendisidir.
    files.push((
        "src/lib.rs".into(),
        "pub struct Foo;\n\nimpl Foo {\n    pub fn new() -> Self {\n        Foo\n    }\n}\n".into(),
    ));
    let mut hub = String::from("def hub():\n    return 0\n");
    for index in 0..30 {
        hub.push_str(&format!("\n\ndef caller_{index:02}():\n    return hub()\n"));
    }
    files.push(("app/hub.py".into(), hub));
    files
}

/// Fikstürü yazar, indeksler ve MCP sunucusunu başlatır.
struct Server {
    _dir: tempfile::TempDir,
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    project: String,
    next_id: u64,
}

impl Server {
    fn start() -> Self {
        std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
        let dir = tempfile::tempdir().expect("tempdir");
        for (path, content) in fixture_files() {
            let full = dir.path().join(&path);
            std::fs::create_dir_all(full.parent().expect("parent")).expect("mkdir");
            std::fs::write(&full, content).expect("write fixture");
        }
        let project = dir.path().to_string_lossy().to_string();
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            ccm_core::index_directory(
                &project,
                Some(&dir.path().join("data/ccm_db").to_string_lossy()),
            )
            .await
            .expect("index fixture");
        });
        let mut child = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"))
            .env("CCM_DISABLE_EMBEDDER", "1")
            .env("CCM_MCP_DEBUG", "0")
            .env("CCM_AUTO_REFRESH", "0")
            .env("CCM_ALLOWED_ROOTS", dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ccm-mcp");
        let stdin = child.stdin.take().expect("stdin");
        let reader = BufReader::new(child.stdout.take().expect("stdout"));
        let mut server = Self {
            _dir: dir,
            child,
            stdin,
            reader,
            project,
            next_id: 0,
        };
        server.request("initialize", json!({}));
        server
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let request =
            json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        writeln!(self.stdin, "{request}").expect("write request");
        self.stdin.flush().expect("flush");
        let mut line = String::new();
        self.reader.read_line(&mut line).expect("read response");
        serde_json::from_str(line.trim()).unwrap_or_else(|error| panic!("bad JSON {error}: {line}"))
    }

    /// Aracı çağırır: (metin, isError).
    fn call(&mut self, tool: &str, mut arguments: Value) -> (String, bool) {
        arguments["project_path"] = json!(self.project);
        let response = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        assert!(
            response.get("error").is_none(),
            "{tool} JSON-RPC error: {response}"
        );
        // Metin tazelik satırıyla başlar; birden çok içerik öğesi birleştirilir.
        let text = response["result"]["content"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        (text, response["result"]["isError"] == true)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn targets_accept_path_line_and_unique_names() {
    let mut server = Server::start();

    let (text, is_error) = server.call("find_usages", json!({"target": "helper"}));
    assert!(is_error, "an ambiguous name must be an error: {text}");
    assert!(
        text.contains("app/util.py:1") && text.contains("app/other.py:1"),
        "{text}"
    );

    let (text, is_error) = server.call("find_usages", json!({"target": "app/util.py:1"}));
    assert!(!is_error, "{text}");

    let (text, is_error) = server.call("find_usages", json!({"target": "Engine.start"}));
    assert!(!is_error, "{text}");
    assert!(
        text.contains("run"),
        "engine.start() in run may call Engine.start: {text}"
    );

    let (text, is_error) = server.call("find_usages", json!({"target": "app/core.py:8"}));
    assert!(
        !is_error,
        "a blank line inside a class resolves to the class: {text}"
    );
    assert!(text.contains("usages of `Engine`"), "{text}");

    let (text, is_error) = server.call("find_usages", json!({"target": "app/core.py:3"}));
    assert!(is_error, "{text}");
    assert!(text.contains("no symbol at app/core.py:3"), "{text}");
}

#[test]
fn usages_are_compact_lines_without_node_ids() {
    let mut server = Server::start();
    let (text, is_error) = server.call("find_usages", json!({"target": "app/models.py:2"}));
    assert!(!is_error, "{text}");
    assert!(
        text.contains("- Function: save · app/models.py:7-8 · calls"),
        "one compact line per usage: {text}"
    );
    assert!(!text.contains(":symbol:"), "no node IDs: {text}");
    assert!(!text.contains("Score:"), "no scores: {text}");
}

#[test]
fn max_tokens_caps_output_and_reports_the_rest() {
    let mut server = Server::start();
    let (text, is_error) = server.call(
        "find_usages",
        json!({"target": "hub", "max_tokens": 200, "limit": 50}),
    );
    assert!(!is_error, "{text}");
    assert!(
        text.len() <= 1000,
        "budget of ~200 tokens: {} chars\n{text}",
        text.len()
    );
    assert!(text.contains("more not shown (max_tokens=200)"), "{text}");
    assert!(
        text.contains("- Function: caller_00 · app/hub.py:"),
        "usages follow file position, so the budget keeps the first ones: {text}"
    );

    let (text, is_error) =
        server.call("find_usages", json!({"target": "hub", "max_tokens": "200"}));
    assert!(
        is_error,
        "a string budget is an input error, not the default: {text}"
    );
    assert!(
        text.contains("'max_tokens' must be a positive integer"),
        "{text}"
    );
}

#[test]
fn printed_locations_files_and_nested_members_work_as_targets() {
    let mut server = Server::start();
    for (target, expected) in [
        ("app/core.py:13-17", "Function `run`"),
        ("app/other.py:1 Function helper", "Function `helper`"),
        ("app/cli.py", "File · app/cli.py:"),
        ("Outer.Inner", "Class `Inner`"),
        ("Foo", "Struct `Foo`"),
    ] {
        let (text, is_error) = server.call("explain", json!({"target": target}));
        assert!(!is_error, "{target}: {text}");
        assert!(text.contains(expected), "{target}: {text}");
    }
    let (text, _) = server.call("explain", json!({"target": "app/cli.py"}));
    assert!(
        text.contains("- Function: main · app/cli.py:4-5"),
        "a file lists its top-level symbols: {text}"
    );

    let (text, is_error) = server.call(
        "trace_call_chain",
        json!({"from": "./app/core.py:function_definition:symbol:0000000000000000:0", "to": "app/util.py:1"}),
    );
    assert!(is_error, "an unknown node ID is an error: {text}");
    assert!(text.contains("is not in the current index"), "{text}");
}

#[test]
fn budget_counts_characters_not_bytes() {
    let mut server = Server::start();
    let (text, is_error) = server.call("explain", json!({"target": "cjk", "max_tokens": 300}));
    assert!(!is_error, "{text}");
    assert!(
        text.len() > text.chars().count() + 500,
        "the body is multi-byte: {text}"
    );
    assert!(
        text.contains("- Function: use_3 · app/cjk_users.py:"),
        "callers fit a 1,200-character budget: {text}"
    );
    assert!(!text.contains("more (raise max_tokens)"), "{text}");
}

#[test]
fn explain_returns_definition_callers_callees_and_tests_in_one_call() {
    let mut server = Server::start();
    let (text, is_error) = server.call("explain", json!({"target": "run"}));
    assert!(!is_error, "{text}");
    assert!(
        !text.contains("members:"),
        "local variables are not members: {text}"
    );
    for expected in [
        "Function `run` · app/core.py:13-17",
        "def run():",
        "callers:",
        "main · app/cli.py",
        "callees:",
        "Class: Engine",
        "tests:",
        "test_run · tests/test_core.py",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text}");
    }
}

#[test]
fn explain_handles_a_symbol_without_callers() {
    let mut server = Server::start();
    // `app.util.helper` import edildiği için `app/other.py` içindeki `helper`'ı kimse çağırmaz.
    let (text, is_error) = server.call("explain", json!({"target": "app/other.py:1"}));
    assert!(!is_error, "{text}");
    assert!(text.contains("0 callers"), "{text}");
    assert!(
        !text.contains("callers:"),
        "an empty list is not printed: {text}"
    );
}

#[test]
fn explain_lists_class_members() {
    let mut server = Server::start();
    let (text, is_error) = server.call("explain", json!({"target": "Engine"}));
    assert!(!is_error, "{text}");
    assert!(text.contains("members:"), "{text}");
    assert!(
        text.contains("- Function: stop · app/core.py:9-10"),
        "{text}"
    );
}

#[test]
fn map_lists_files_by_their_most_used_symbols_within_budget() {
    let mut server = Server::start();
    let (text, is_error) = server.call("map", json!({"max_tokens": 300}));
    assert!(!is_error, "{text}");
    let header = text
        .lines()
        .find(|line| !line.is_empty() && !line.starts_with("_Index"))
        .expect("map header");
    assert!(header.contains("files,"), "{text}");
    // `run` diğer dosyalardan çağrılır ve import edilir; `hub`'ın çağıranları kendi dosyasında.
    let core = text
        .find("app/core.py — run(")
        .unwrap_or_else(|| panic!("run is used from other files: {text}"));
    let other = text
        .find("app/other.py")
        .unwrap_or_else(|| panic!("every file is listed: {text}"));
    assert!(core < other, "most used first: {text}");
    assert!(text.len() <= 1_500, "{} chars: {text}", text.len());
}

#[test]
fn map_with_an_unknown_prefix_says_so() {
    let mut server = Server::start();
    let (text, is_error) = server.call("map", json!({"path": "nope"}));
    assert!(is_error, "{text}");
    assert!(text.contains("no indexed code under nope"), "{text}");

    let (text, is_error) = server.call("map", json!({"path": "."}));
    assert!(!is_error, "`.` is the whole project: {text}");
    assert!(text.contains("app/core.py — run("), "{text}");
}

#[test]
fn tool_list_is_lean() {
    let mut server = Server::start();
    let response = server.request("tools/list", json!({}));
    let tools = response["result"]["tools"].as_array().expect("tools");
    let bytes = serde_json::to_string(&response["result"])
        .expect("serialize tools/list")
        .len();
    assert!(bytes <= 6_500, "tools/list is {bytes} bytes");
    let mut names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("name"))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "diff_context",
            "explain",
            "find_nodes",
            "find_usages",
            "impact_of_change",
            "index_now",
            "index_project",
            "map",
            "search_code",
            "trace_call_chain"
        ]
    );
    for tool in tools {
        let description = tool["description"].as_str().unwrap_or_default();
        assert!(
            description.chars().count() <= 220,
            "{} description has {} chars",
            tool["name"],
            description.chars().count()
        );
    }
}
