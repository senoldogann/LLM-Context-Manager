# Rust Syntax Graph and Honest Name-Match Labels (M3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Answers for non-Python code stop presenting name matches as facts, and Rust references are resolved from the syntax tree, so `find_usages`, `explain`, `impact_of_change` and `map` are correct for Rust projects.

**Architecture:** Mirrors M1 (Python). A Rust fact extractor records call sites, `use` bindings, trait bases and type names on each node as `SyntaxFacts { language: Rust }`. A Rust module model maps files and inline `mod` blocks to module paths, using the `Cargo.toml` files that are already indexed as `DataFile` nodes. A Rust resolver turns the facts into the existing edge types. Languages without a syntax extractor keep name matching, but their edges are relabelled as inferred.

**Tech Stack:** Rust 1.99, tree-sitter 0.26, tree-sitter-rust 0.24, petgraph.

**Spec:** this plan. It continues `2026-10-01-python-syntax-graph.md` (M1) and `2026-10-02-token-efficient-answers.md` (M2).

## Roadmap

- **M3 (this plan):** honest labels for name-matched edges in every language without syntax facts; Rust syntax graph.
- **M4:** TypeScript/JavaScript syntax graph (ES modules, CommonJS, classes).
- **M5:** edge accuracy against language servers (L2), and the agent A/B benchmark (L3, which needs an API budget approval).

## Findings that motivate this plan

1. In the 12 languages without syntax facts, `resolve_source_references` (`core/src/graph/mod.rs`) labels a unique-name match `Calls` and a mention of a class `Imports`. An agent reads both as facts. For example, `x.len()` links to the only project `fn len`, and a type mentioned in an expression becomes an "import".
2. `default_edge_weights` (`core/src/engine/hybrid.rs`) has no weight for `CallInferred` or `References`. M1's Python edges of those types add nothing to hybrid search ranking.
3. Rust has no `enum` or `trait` nodes: `classify_rust_node` ignores `enum_item` and `trait_item`, so references to them are lost.
4. Rust calls inside macros (`assert_eq!(f(), 1)`, `format!("{}", g())`) are token trees in tree-sitter, not call expressions. They need a token scan to keep recall.

## Design decisions

- **D1. Honest name-match labels.** For sources with `ReferenceFacts::Lexical`:
  - a call-like token whose name has exactly one definition in the project, or exactly one in the same file, becomes `CallInferred`;
  - several definitions in the same file stay `CallAmbiguous`;
  - a non-call mention of a Class, Struct, Enum, Trait or Module becomes `References`;
  - an ambiguous mention produces no edge.

  The `CallsInferred` label becomes `calls (inferred: the only definition with that name)`, which fits both Python and name matching.
- **D2. Search weights.** Add `(CallInferred, 1.00)` and `(References, 0.60)` to `default_edge_weights`, matching the weights that `Calls` and `Imports` had before D1. Name-matched languages therefore rank exactly as before, and Python's `References` and `CallInferred` edges start to count. Both eval gates must stay at 100%.
- **D3. Node kinds.** Add `NodeType::Enum` and `NodeType::Trait`.
  - `enum_item` → Enum; `trait_item` → Trait.
  - `function_signature_item` (a trait method without a body) → Function, contained by the trait.
  - `is_reference_target_type`, the `map` symbol list and `symbols_named` owner types include Enum and Trait.
- **D4. Rust facts (`SyntaxLanguage::Rust`).** Paths are stored with `.` separators, so `crate::a::B::f()` becomes `Member { qualifier: "crate.a.B", name: "f" }`.
  - **Calls:**
    - `f()` → `Bare("f")`.
    - A path call `a::b::f()` → `Member`. Turbofish is unwrapped.
    - `self.f()` and `Self::f()` → `SelfMember("f")`.
    - Any other method call `x.f()` or `g().f()` → `Chained("f")`: the receiver's type is unknown.
    - A struct expression `Foo { .. }` → `Bare("Foo")` or `Member` for a path: construction counts as a call, as in Python.
    - Macro token trees are scanned for an identifier directly followed by a parenthesised token tree; a preceding `::` path or `.` receiver selects `Member` or `Chained`.
  - **`use` bindings:** stored as `ImportBinding { local, module, symbol }` with the raw path (`crate.util`, `super.x`, `app_core.util`); they are resolved later with the owner's module. The forms are:

    | Rust | `local` | `module` | `symbol` |
    |---|---|---|---|
    | `use a::b;` | `b` | `a` | `Some("b")` |
    | `use a::b as c;` | `c` | `a` | `Some("b")` |
    | `use a::{self, C as D, e::*};` | `a` | parent of `a` | `Some("a")` |
    | | `D` | `a` | `Some("C")` |
    | | `*` | `a.e` | `Some("*")` |
    | `use a::*;` | `*` | `a` | `Some("*")` |

    `pub use` produces the same bindings; other modules see them as re-exports.
  - **Bases:** the trait of `impl Trait for Type`, and the supertraits of a `trait`.
  - **Names:** type identifiers and `::` paths in reference positions: signatures, fields, `let` types, generic arguments and bounds, and value paths that are not calls. Plain identifiers in value position are skipped, because they are mostly locals.
  - **Owners:**
    - `function_item` holds its signature and body; nested items are their own nodes.
    - `impl_item`, `struct_item`, `enum_item` and `trait_item` hold their bases and names.
    - The file node, and each inline `mod_item`, hold their `use` declarations and the calls in `const`/`static` initialisers.
- **D5. Rust modules (`RustCrates`).**
  - **Crates:** each `Cargo.toml` `DataFile` with a `[package] name` defines its crates.
    - The lib root `src/lib.rs` is addressable from other crates as the package name with `-` replaced by `_`.
    - The bin root `src/main.rs` and every `src/bin/*.rs`, `tests/*.rs`, `benches/*.rs` and `examples/*.rs` file is its own crate root, and sees the lib crate by that name.
    - A `.rs` file under no `Cargo.toml` is a crate root of its own.
  - **Module path of a file:** relative to its root directory, without `.rs`, with a trailing `mod` dropped (`src/a/b.rs` and `src/a/b/mod.rs` → `[a, b]`; `lib.rs` and `main.rs` → `[]`). An inline `mod x { }` adds `x`.
- **D6. Rust scopes and resolution.**
  - **Bare names:** the scope of a name is checked in this order:
    1. items and `use` bindings of the enclosing function (for nested items and local `use`);
    2. items and `use` bindings of the enclosing module (the file, or the inline mod);
    3. glob imports of that module.

    There is no fallback to other modules and no project-wide unique-name guess. An unresolved name is external (std, prelude, extern crates) and produces no edge.
  - **Paths:**
    - The first segment is `crate`, `self`, `super`, `Self`, a workspace crate name, or a name in scope; anything else is external.
    - Later segments step into modules (file modules, inline mods, `pub use` re-exports, at most 4 hops) or into a type's associated items.
    - Associated items are the methods of the `impl` blocks named after the type in the same crate, plus the default methods of the traits it implements.
    - A path that reaches a type but not a member (an enum variant, an associated const) is a `References` edge to the type.
  - **Edges:**
    - `Bare` and `Member` give `Calls` for one target and `CallAmbiguous` for several.
    - `SelfMember` resolves against the enclosing impl's type, and falls back to `Chained` when nothing matches.
    - `Chained` resolves to the methods (Contains children of Rust impl or trait nodes) named the same, with at most 5 candidates; the result is `CallAmbiguous`, never `Calls`.
    - Bases give `Inherits`, names give `References`, and module-level `use` bindings give `Imports` edges from the File or mod node to the imported item (a module import points at its File node). Glob imports give no edges.
- **D7. Incremental refresh.** `SyntaxFacts::mentions_any` also checks the components of `ImportBinding::module`.
  - **Re-exports:** a changed Rust file's affected names include its `pub use` locals.
  - **`Cargo.toml`:** a changed `Cargo.toml` adds the old and new crate names.
  - **Impl blocks:** a changed impl block adds its type's name, because method resolution goes through the type.

  Incremental edges must equal a full rebuild.
- **D8. Index schema 8.**
- **D9. Measurement.**
  - **Edge counts:** count edges by relation for Rust sources on this repository and on the serde corpus, with v0.4.0 (`e9a6920`) against the M3 head.
  - **Spot check:** check 30 randomly sampled `calls` edges per repository by reading the call site. Report it as a spot check by the implementer, not as a benchmark.
  - **Eval gates:** run the structural gate (`golden_tasks.v3.ccm.json` without `search_code`) and the synthetic gate (`eval/fixtures/golden_tasks.synthetic.json`); both stay at 100%.

## Global Constraints

- Toolchain: `PATH="$HOME/.cargo/bin:$PATH" cargo …` (Rust 1.99.0); `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass before every commit.
- Code comments in Turkish; no default parameter values; explicit errors, no silent fallbacks; strict types; no `unwrap` outside tests.
- `INDEX_SCHEMA_VERSION` 7 → 8.
- No edge is labelled `calls` unless a scope rule (Python or Rust) resolved it; name matches are `calls (inferred …)` or `may call`.
- CI eval gates (`.github/workflows/eval.yml`) stay at 100%. Any golden-task expectation change needs a per-task reason in the commit message.
- No unmeasured claims. Spot checks are labelled as such.
- One commit per task, with tests.

## Review Focus

1. **Workspace crates:** a crate whose package name has `-` (`app-core` → `app_core`) is used from another workspace crate. The `use` resolves across crates; `std::`, `serde_json::` and other external paths produce no edge, never a wrong project edge. Test: `rust_paths_resolve_across_workspace_crates_and_reexports` (Task 4).
2. **Tests module:** inside `#[cfg(test)] mod tests { use super::*; … }`, calls resolve to the parent module's items. Test: `rust_test_modules_see_parent_items_through_glob_imports` (Task 4).
3. **Macros:** calls inside macros (`assert_eq!(f(), 1)`, `println!("{}", g())`) produce edges. Test: the same Task 4 test, plus a facts test in Task 2.
4. **Same name, two modules:** with `util::helper` and `other::helper`, only the imported or qualified one gets `calls`. Test: `rust_calls_resolve_through_scopes_use_and_impls` (Task 4).
5. **Method on a value:** `x.len()` with a unique project method named `len` is `may call`, never `calls`. Test: the same Task 4 test.

---

## File structure

| File | Responsibility |
|---|---|
| `core/src/graph/mod.rs` | `NodeType::{Enum, Trait}`; lexical relabel (D1); dispatch of Rust facts; refresh hooks (D7) |
| `core/src/graph/references.rs` | `SyntaxLanguage::Rust`; `mentions_any` covers module path components |
| `core/src/graph/usages.rs` | `CallsInferred` label text |
| `core/src/engine/hybrid.rs` | weights for `CallInferred` and `References` (D2) |
| `core/src/vector/extractor.rs` | Rust node kinds (D3); Rust facts on nodes and on the file node |
| `core/src/vector/rust_facts.rs` (new) | Rust fact extraction (D4) |
| `core/src/graph/rust_modules.rs` (new) | crates, module paths, module scopes (D5) |
| `core/src/graph/resolve_rust.rs` (new) | Rust resolution rules and edges (D6) |
| `core/src/graph/map.rs`, `core/src/graph/mod.rs` (`symbols_named`) | Enum and Trait in maps and member owners |
| `core/src/lib.rs` | schema 8 |
| `core/tests/lexical_labels_test.rs` (new) | D1 on a Go fixture |
| `core/tests/rust_references_test.rs` (new) | D4–D6 on a two-crate workspace fixture |
| `core/tests/live_index_parity_test.rs` | Rust incremental parity (D7) |
| `SKILL.md`, `README.md`, `README.tr.md`, `benchmarks/README.md` | relation wording; Rust edge measurement |

---

### Task 1: Honest labels for name matches, and search weights

**Files:**
- Modify: `core/src/graph/mod.rs` (`resolve_source_references`)
- Modify: `core/src/graph/usages.rs` (`UsageRelation::label`)
- Modify: `core/src/engine/hybrid.rs` (`default_edge_weights`)
- Test: `core/tests/lexical_labels_test.rs` (new)

**Interfaces:**
- Produces: lexical sources emit `CallInferred`, `CallAmbiguous` and `References` only.

- [ ] **Step 1: Write the failing test**

```rust
//! Sözdizimi çıkarıcısı olmayan dillerde ad eşleşmesi kesin ilişki gibi etiketlenmez.

use anyhow::Result;
use ccm_core::graph::{CodeGraph, EdgeType};
use petgraph::visit::EdgeRef;

/// Go fikstürü: `Run` tek tanımlı `Helper`'ı çağırır ve `Engine` türünü anar.
const FILES: &[(&str, &str)] = &[
    ("pkg/helper.go", "package pkg\n\nfunc Helper() int {\n\treturn 1\n}\n\ntype Engine struct{}\n"),
    ("pkg/run.go", "package pkg\n\nfunc Run() int {\n\tvar e Engine\n\t_ = e\n\treturn Helper()\n}\n"),
];

#[tokio::test]
async fn name_matches_are_inferred_not_resolved() -> Result<()> {
    let dir = tempfile::tempdir()?;
    for (path, content) in FILES {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().expect("parent"))?;
        std::fs::write(full, content)?;
    }
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = dir.path().to_string_lossy().to_string();
    ccm_core::index_directory(&project, None).await?;
    let artifacts = ccm_core::resolve_index_artifacts(&project, None)?;
    let graph = CodeGraph::from_file(&artifacts.graph_path.to_string_lossy())?;
    let edge = |from: &str, to: &str| -> Vec<EdgeType> {
        let source = graph.find_nodes_by_name(from)[0];
        let target = graph.find_nodes_by_name(to)[0];
        graph
            .graph
            .edges_connecting(source, target)
            .map(|edge| edge.weight().clone())
            .collect()
    };
    assert_eq!(edge("Run", "Helper"), vec![EdgeType::CallInferred]);
    assert_eq!(edge("Run", "Engine"), vec![EdgeType::References]);
    Ok(())
}
```

- [ ] **Step 2: Run it.** Command: `cargo test -p ccm-core --test lexical_labels_test`. Expected: FAIL, because the edges are `[Calls]` and `[Imports]`.
- [ ] **Step 3: Implement.**
  - In `resolve_source_references`, map an unambiguous call to `EdgeType::CallInferred` and an ambiguous call to `CallAmbiguous`.
  - Map an unambiguous non-call mention of a type to `References`, and drop ambiguous mentions (`continue`).
  - Set the `CallsInferred` label to `"calls (inferred: the only definition with that name)"`.
  - Add `(EdgeType::CallInferred, 1.00)` and `(EdgeType::References, 0.60)` to `default_edge_weights`.
- [ ] **Step 4: Run the tests.**
  - Run `cargo test -p ccm-core --test lexical_labels_test`, then `cargo test --workspace`.
  - Update every existing assertion that encoded a name match as `Calls` or `Imports` (find them with `rg -n "EdgeType::(Calls|Imports)" core/src core/tests mcp/tests`); each update names the D1 rule in a comment.
- [ ] **Step 5: Run both eval gates locally** (commands in Task 6, Step 3). Both stay at 100%.
- [ ] **Step 6: Commit** with the message `fix(graph): label name matches as inferred, not as resolved calls`.

### Task 2: Rust node kinds and fact extraction

**Files:**
- Create: `core/src/vector/rust_facts.rs`
- Modify: `core/src/vector/mod.rs` (`pub mod rust_facts;`), `core/src/vector/extractor.rs`, `core/src/graph/references.rs`, `core/src/graph/mod.rs` (NodeType), `core/src/lib.rs` (schema 8)
- Test: `core/tests/rust_references_test.rs` (new; fixture below)

**Interfaces:**
- Produces:
  - `rust_facts::function_facts(definition: Node, source: &str) -> SyntaxFacts`
  - `rust_facts::item_facts(definition: Node, source: &str) -> SyntaxFacts`
  - `rust_facts::module_facts(scope: Node, source: &str) -> SyntaxFacts`
  - `SyntaxLanguage::Rust`, `NodeType::Enum`, `NodeType::Trait`

The fixture is shared by Tasks 2–5. It is a two-crate workspace:

```rust
const FIXTURE: &[(&str, &str)] = &[
    ("Cargo.toml", "[workspace]\nmembers = [\"app_core\", \"app_cli\"]\n"),
    ("app_core/Cargo.toml", "[package]\nname = \"app-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    ("app_core/src/lib.rs", "pub mod engine;\npub mod other;\npub mod util;\n\npub use engine::Engine;\n"),
    (
        "app_core/src/util.rs",
        "/// helper() bir yorumda: kenar değil\npub fn helper() -> u32 {\n    1\n}\n\npub enum Mode {\n    Fast,\n    Slow,\n}\n\nimpl Mode {\n    pub fn len(&self) -> usize {\n        0\n    }\n}\n",
    ),
    ("app_core/src/other.rs", "pub fn helper() -> u32 {\n    2\n}\n"),
    (
        "app_core/src/engine.rs",
        r#"use crate::util::helper;

pub struct Engine {
    pub mode: crate::util::Mode,
}

pub trait Runner {
    fn run(&self) -> u32;
}

impl Engine {
    pub fn new() -> Self {
        Engine { mode: crate::util::Mode::Fast }
    }

    pub fn start(&self) -> u32 {
        self.stop();
        helper()
    }

    pub fn describe(&self) -> usize {
        let text = "helper()";
        text.len()
    }

    fn stop(&self) {}
}

impl Runner for Engine {
    fn run(&self) -> u32 {
        self.start()
    }
}
"#,
    ),
    ("app_cli/Cargo.toml", "[package]\nname = \"app-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    (
        "app_cli/src/main.rs",
        r#"use app_core::util::{self, Mode};
use app_core::Engine;

fn main() {
    let engine = Engine::new();
    engine.start();
    util::helper();
    let _mode = Mode::Slow;
    println!("{}", compute());
}

fn compute() -> u32 {
    serde_json::to_string(&1).map(|text| text.len() as u32).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes() {
        assert_eq!(compute(), 1);
    }
}
"#,
    ),
];
```

- [ ] **Step 1: Write the failing test** `rust_facts_capture_calls_uses_bases_and_macros`. Index the fixture, load the graph, and assert:
  - `start` has calls `[SelfMember("stop"), Bare("helper")]`;
  - `describe` has `[Chained("len")]` and no `Bare("helper")` (strings are not calls);
  - `main` has `Member { qualifier: "Engine", name: "new" }`, `Chained("start")`, `Member { qualifier: "util", name: "helper" }`, `Member { qualifier: "Mode", name: "Slow" }` and `Bare("compute")` (from `println!`);
  - `computes` has `Bare("compute")` (from `assert_eq!`);
  - `new` has `Bare("Engine")` (struct expression) and `Member { qualifier: "crate.util.Mode", name: "Fast" }`;
  - the main.rs File node has imports `{util → module app_core, symbol util}`, `{Mode → app_core.util, Mode}` and `{Engine → app_core, Engine}`;
  - the `tests` mod node has `{* → super, *}`;
  - the second `Engine` impl node has bases `[Bare("Runner")]`;
  - `Mode` is a `NodeType::Enum` node and `Runner` a `NodeType::Trait` node.
- [ ] **Step 2: Run it.** Expected: FAIL; Rust facts are `Lexical`.
- [ ] **Step 3: Implement.**
  - Write `rust_facts.rs` with the tree-sitter-rust kinds: `call_expression`, `generic_function`, `scoped_identifier`, `field_expression`, `struct_expression`, `macro_invocation`/`token_tree`, `use_declaration`, `scoped_use_list`, `use_list`, `use_as_clause`, `use_wildcard`, `type_identifier`, `scoped_type_identifier` and `generic_type`.
  - Before relying on a kind, verify it with a dump of the fixture tree (`node.to_sexp()`) in a scratch test.
  - Classify `enum_item`, `trait_item` and `function_signature_item`, and attach facts in `reference_facts` and for the file node.
  - Set `INDEX_SCHEMA_VERSION = 8` with a one-line reason comment.
- [ ] **Step 4: Run the tests.** Run the new test, then `cargo test --workspace`.
- [ ] **Step 5: Commit** with the message `feat(index): extract Rust call sites, use bindings and trait bases from the syntax tree`.

### Task 3: Rust module model

**Files:**
- Create: `core/src/graph/rust_modules.rs`
- Test: `core/tests/rust_references_test.rs`

**Interfaces:**
- Produces:
  - `RustCrates::new(graph: &CodeGraph) -> RustCrates`
  - `RustCrates::module_of_file(&self, file_id: &str) -> RustModule`
  - `RustCrates::lib_named(&self, name: &str) -> Option<&RustCrate>`
  - `RustCrates::file_of(&self, krate: &RustCrate, path: &[String]) -> Option<String>`
  - `RustModule { krate: usize, path: Vec<String> }`

- [ ] **Step 1: Write the failing test** `rust_files_map_to_crates_and_module_paths`. Using the fixture, assert:
  - `./app_core/src/lib.rs` → crate `app_core`, path `[]`;
  - `./app_core/src/util.rs` → `[util]`;
  - `./app_cli/src/main.rs` → crate `app_cli` (bin), path `[]`;
  - `lib_named("app_core")` points at `./app_core/src/lib.rs`;
  - a loose file `scratch.rs` at the project root is its own crate root.
- [ ] **Step 2: Run it.** Expected: FAIL, because the module does not exist yet.
- [ ] **Step 3: Implement.**
  - Parse `[package]` / `name = "…"` line by line from `Cargo.toml` contents, without a TOML dependency. A `name` outside `[package]` is ignored, and a `Cargo.toml` without a package name defines no crate.
  - Implement D5 path rules. Expose the API through `pub(crate)`, plus a `pub fn rust_module_of(graph: &CodeGraph, file_id: &str) -> (String, Vec<String>)` for the test.
- [ ] **Step 4: Run the tests.** Expected: PASS.
- [ ] **Step 5: Commit** with the message `feat(graph): map Rust files to workspace crates and module paths`.

### Task 4: Rust resolver

**Files:**
- Create: `core/src/graph/resolve_rust.rs`
- Modify: `core/src/graph/mod.rs` (`resolve_references` dispatch)
- Test: `core/tests/rust_references_test.rs`

**Interfaces:**
- Consumes: Tasks 2 and 3.
- Produces: `resolve_rust::rust_references(graph: &CodeGraph, crates: &RustCrates, source_idx: NodeIndex, facts: &SyntaxFacts) -> Vec<(NodeIndex, NodeIndex, EdgeType)>`

- [ ] **Step 1: Write the failing tests.** Each line is `from → to: expected edge types`. The helpers `node(file, name)` and `member(file, owner, name)` work as in `python_references_test.rs`; for `member`, the owner of an impl method is the impl node named after the type.
  - **`rust_calls_resolve_through_scopes_use_and_impls`:**
    - `start → util::helper: [Calls]`; `start → other::helper: []`
    - `start → Engine::stop: [Calls]`
    - `describe → util::helper: []`; `describe → Mode::len: [CallAmbiguous]`
    - `new → Engine (struct): [Calls]`; `new → Mode (enum): [References]`
    - `run → Engine::start: [Calls]`
    - `impl Runner for Engine → Runner: [Inherits]`
  - **`rust_paths_resolve_across_workspace_crates_and_reexports`:**
    - `main → Engine::new: [Calls]` (through `app_core` and `pub use engine::Engine`)
    - `main → Engine::start: [CallAmbiguous]`
    - `main → util::helper: [Calls]`; `main → other::helper: []`
    - `main → Mode: [References]`
    - main.rs File `→ Engine (struct): [Imports]`; main.rs File `→ Mode: [Imports]`
    - lib.rs File `→ Engine (struct): [Imports]`
    - `compute`: no edge to any project node except `Mode::len` (`may call`)
  - **`rust_test_modules_see_parent_items_through_glob_imports`:**
    - `computes → compute: [Calls]`
    - `main → compute: [Calls]` (inside `println!`)
- [ ] **Step 2: Run them.** Expected: FAIL; there are no Rust syntax edges yet.
- [ ] **Step 3: Implement D6.**
  - Re-export following is capped at 4 hops, like `MAX_IMPORT_DEPTH` in `resolve.rs`.
  - `possible` for Rust filters to members of Rust impl or trait nodes.
  - Dispatch `SyntaxLanguage::Rust` in `resolve_references`, and build `RustCrates` once per call, as `PythonModules` is built.
- [ ] **Step 4: Run the tests.** Run the three tests, then `cargo test --workspace`, then both eval gates.
- [ ] **Step 5: Commit** with the message `feat(graph): resolve Rust calls through scopes, use paths, crates and impl blocks`.

### Task 5: Rust incremental refresh

**Files:**
- Modify: `core/src/graph/references.rs` (`mentions_any`), `core/src/graph/mod.rs` (`reference_target_names`), `core/src/graph/resolve_rust.rs` (affected-name expansion)
- Test: `core/tests/live_index_parity_test.rs`

- [ ] **Step 1: Write the failing test.** Add `rust_incremental_edges_equal_a_full_rebuild`, following the existing Python parity case: index the fixture, then apply each edit with `update_index`. After each edit, the edge triples equal those of a fresh `index_directory` over the same files.
  1. In `lib.rs`, replace `pub use engine::Engine;` with `pub use other::helper as engine_helper;`.
  2. Rename `helper` in `util.rs` to `assist`.
  3. Rename the package in `app_core/Cargo.toml` to `app-kernel`.
  4. Add a second `impl Engine` block with `fn stop(&self) {}` in `other.rs`.
- [ ] **Step 2: Run it.** Expected: FAIL on at least edits 1 and 3.
- [ ] **Step 3: Implement D7.**
- [ ] **Step 4: Run the tests.** Run the parity test, then `cargo test --workspace`.
- [ ] **Step 5: Commit** with the message `fix(index): refresh Rust callers when re-exports, impls or crate names change`.

### Task 6: Surfaces, documents, measurement

**Files:**
- Modify: `core/src/graph/map.rs`, `core/src/graph/mod.rs` (`symbols_named` owner types: add Enum and Trait), `SKILL.md`, `README.md`, `README.tr.md`, `benchmarks/README.md`
- Test: `mcp/tests/token_budget_e2e_test.rs` (an `explain` on a trait, and `map` listing an enum)

- [ ] **Step 1: Write the failing e2e test.** Add a small Rust file to the fixture with a trait `Store` and an enum `Kind`.
  - `explain {target:"Store"}` returns `Trait \`Store\``.
  - `map` lists `Kind`.
- [ ] **Step 2: Implement.** Then update the documents:
  - **SKILL.md:** the relations paragraph reads "Python and Rust are resolved at syntax level; other languages are matched by name, and those matches are labelled `calls (inferred …)` or `references`."
  - **README.md and README.tr.md:** update the Limits paragraph the same way.
- [ ] **Step 3: Measure (D9).**
  - Build both binaries: `git worktree add` a scratch checkout of `e9a6920` in the scratchpad and build it there.
  - Index this repository and `benchmarks/corpus/serde` with each, graph only.
  - Count edges by relation with a scratch script, then spot-check 30 sampled `calls` edges per repository.
  - Run the eval gates:

```bash
jq '.tasks |= map(select(.query.type != "search_code"))' eval/golden_tasks.v3.ccm.json > "$SCRATCH/structural.json"
./target/release/ccm-cli index --path .
./target/release/ccm-cli eval --tasks "$SCRATCH/structural.json" --min-pass-rate 100
./target/release/ccm-cli eval --tasks eval/fixtures/golden_tasks.synthetic.json --min-pass-rate 100
```

  - Publish a "Rust edges (M3)" section in `benchmarks/README.md`: the counts table, the spot check labelled as such, and its limits.
- [ ] **Step 4: Run the full verification.** fmt, clippy, workspace tests, npm tests.
- [ ] **Step 5: Commit** with the message `docs: Rust syntax graph measurement and relation wording`.
- [ ] **Step 6: Final review.** A fresh reviewer on the most capable model reviews the whole branch, followed by one fix pass. Then fast-forward `main` and push it: the user approved pushes to `main`. A release needs a new approval.
