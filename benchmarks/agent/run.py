"""Bir görevi bir kolda çalıştırır: çalışma kopyası, CCM indeksi, ajan, puan ve kayıt.

Kollar aynı modeli, aynı istemi ve aynı yerleşik araçları kullanır. Varsayılan
`preregistered` profil final A/B/C tasarımını korur; `context-first-pilot` yalnız ürün
pilotunda B'yi küçük varsayılan context yüzeyi, C'yi eski full yüzey olarak karşılaştırır.
Ajan repo ve ev dizini dışındaki geçici bir dizinde, geçici HOME/konfigürasyonla çalışır;
kullanıcının ayarları, kancaları, eklentileri, MCP sunucuları ve CLAUDE.md dosyaları yüklenmez.
Her koşuda kimlik kaynağı ve izin verilen araç kümesi doğrulanır; sapma deneyi durdurur.
"""

from __future__ import annotations

import fcntl
import hashlib
import json
import os
import re
import shlex
import shutil
import signal
import subprocess
import tempfile
import time
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import Path
from typing import Literal

from agent import codex
from agent.score import Score, marker_files, score
from agent.tasks import Task, prompt_for
from freshness.jsonio import as_bool, as_float, as_int, as_list, as_object, as_str
from freshness.model import JsonValue
from freshness.repos import WorkspaceError, export_corpus, init_git, run_git

# F yalnız context-first-pilot profilinde: B'nin araçları + `context`'i önce çağırma talimatı.
# Doğal kullanım kollarından (A/B/C) ayrı etiketlenir; onların karşılaştırmasını değiştirmez.
# H yalnız context-first-pilot profilinde ve Claude Code'da: B'nin araçları ve notu + kurulumla
# gelen isteğe bağlı Grep hook'u (`ccm-mcp hook claude-pre-tool-use`). Doğal kullanım değildir.
# D yalnız preregistered profilde (ek 14): B'nin güncel ürün sürümü, `context` + `index_project`
# ve `context` notu. A, B ve C ön kayıttaki tanımlarıyla kalır.
Arm = Literal["A", "B", "C", "D", "E", "F", "H"]
Embedding = Literal["local", "openai"]
# subscription: `claude setup-token` ile üretilen abonelik jetonu; api-key: ücretli API anahtarı.
Auth = Literal["subscription", "api-key"]
# claude: Claude Code (Claude aboneliği ya da API anahtarı); codex: Codex CLI (ChatGPT aboneliği).
AgentName = Literal["claude", "codex"]
# Codex dökümünde kollara kapalı olması gereken öğe türleri (web araması, görsel, alt ajan).
CODEX_FORBIDDEN_ITEMS = ("web_search", "image_generation", "collab_tool_call")
# Claude Code'un `init` olayında bildirdiği kimlik kaynağı: OAuth (abonelik) için `none`.
AUTH_SOURCE: dict[Auth, str] = {"subscription": "none", "api-key": "ANTHROPIC_API_KEY"}
# Sınır ya da aşırı yük hatası ajanın başarısızlığı değildir: koşu kaydedilmez, ölçüm durur.
QUOTA_MARKERS = ("usage limit", "rate limit", "rate_limit", "limit reached", "overloaded")
# Claude Code sınır hatasını sonuç olayında HTTP durumuyla da bildirir; metin değişebilir
# ("You've hit your weekly limit" hiçbir işaretle eşleşmez), durum kodu değişmez.
QUOTA_HTTP_STATUS = 429
# Codex sınır mesajındaki yerel saatle sıfırlanma: "try again at Nov 20th, 2026 1:54 AM".
CODEX_RETRY_AT = re.compile(
    r"try again at (?P<month>[A-Z][a-z]{2}) (?P<day>\d{1,2})(?:st|nd|rd|th), (?P<year>\d{4}) "
    r"(?P<clock>\d{1,2}:\d{2} [AP]M)"
)
# Kimlik doğrulama hatası da (geçersiz ya da iptal edilmiş jeton) ajanın başarısızlığı değildir.
AUTH_MARKERS = (
    "failed to authenticate",
    "invalid bearer token",
    "401 unauthorized",
    "authentication_failed",
    "api key is invalid",
    "not logged in",
    # Codex: deneye özel CODEX_HOME'daki yenileme jetonu iptal edilmiş.
    "could not be refreshed",
)
# Kayıt biçimi: 2'de `index_copy_s` ve `index_build_s` vardır; 1'de (alan yok) `index_s`.
RECORD_VERSION = 3
INDEX_CACHE_SCHEMA = 1
# Ajanın gerçekten başladığını gösteren ilk döküm olayı.
START_EVENTS: dict[AgentName, str] = {"claude": "system/init", "codex": "thread.started"}
# Koşu hatasının akıbeti: `record` başarısız koşu olarak yazılır, `stop` harness'ı durdurur.
Disposition = Literal["record", "stop"]

ProductProfile = Literal["preregistered", "context-first-pilot"]

SERVER = "context-manager"
LEGACY_CCM_TOOLS = (
    "map",
    "explain",
    "find_usages",
    "impact_of_change",
    "search_code",
    "find_nodes",
    "trace_call_chain",
    "diff_context",
    "index_project",
    "index_now",
)
ALL_CCM_TOOLS = ("context", *LEGACY_CCM_TOOLS)
CONTEXT_DEFAULT_TOOLS = ("context", "index_project")
BUILTIN_TOOLS = ("Read", "Grep", "Glob", "Edit", "Write", "Bash")
SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"
OPENAI_EMBEDDING_MODEL = "text-embedding-3-small"
# Preregistered B kolunun kısa navigation notu. Final tasarımda anlamı değişmez.
GRAPH_NOTE = (
    "## Code navigation\n"
    "Use the context-manager MCP tools before reading files: `map` once, then `explain` or "
    "`find_usages` for symbols, and `impact_of_change` before editing a file. Read only the "
    "lines you edit."
)
SEARCH_NOTE = (
    "## Code search\n"
    "Use the context-manager `search_code` MCP tool to find code before reading files. Read "
    "only the lines you need."
)
CONTEXT_NOTE = (
    "## Code navigation\n"
    "For who-calls / where-used questions, call `context` first: it lists every call site and "
    "checks completeness, replacing grep + read."
)


# F kolunun ek talimatı: ajan önce `context`'i çağırmalı (zorunlu kullanım ölçümü).
FORCED_NOTE = (
    "\nCall the context-manager `context` tool before any other tool, with the task's "
    "question as the query."
)


class RunError(RuntimeError):
    """Koşu hatalarının ortak türü; kaydedilip kaydedilmeyeceğini `disposition` belirler."""


class SetupError(RunError):
    """Altyapı kurulamadı (çalışma kopyası, indeks önbelleği, kimlik, ajanın başlaması).

    Ajanın sonucu değildir: kayıt yazılmaz, harness durur; sorun giderilince aynı komut
    eksik koşuyu yeniden dener.
    """


class IndexCacheError(SetupError):
    """İndeks önbelleği kurulamadı, vektörsüz kuruldu ya da `entry.json` girdilerle uyuşmuyor."""


class AgentStartError(SetupError):
    """Ajan başlangıç olayına (Claude `init`, Codex `thread.started`) ulaşmadan bitti."""


class AgentTimeout(RunError):
    """Ajan süre sınırında bitmedi: ajanın sonucudur, başarısız koşu olarak kaydedilir."""


class TranscriptError(RunError):
    """Ajan başladı ama dökümü beklenen biçimde değil: başarısız koşu olarak kaydedilir."""


class QuotaError(RuntimeError):
    """Abonelik ya da hız sınırı: koşu kaydedilmez; sınır sıfırlanınca aynı komut sürdürür.

    `resets_at`, ajanın bildirdiği sıfırlanma anıdır (Unix saniyesi); bildirmediyse None.
    """

    def __init__(self, message: str, resets_at: float | None) -> None:
        super().__init__(message)
        self.resets_at = resets_at


class AuthError(RuntimeError):
    """Kimlik doğrulanamadı (geçersiz ya da iptal edilmiş jeton): koşu kaydedilmez, ölçüm durur."""


class IsolationError(RuntimeError):
    """Koşu deneyin yalıtım koşullarını bozdu; kayıt yazılmaz ve ölçüm durur.

    Kimlik kaynağı ANTHROPIC_API_KEY değilse ücret kullanıcının abonelik kotasına yazılabilir;
    araç kümesi kolunkiyle aynı değilse ya da üst dizinde CLAUDE.md varsa kollar karşılaştırılamaz.
    """


@dataclass(frozen=True)
class Settings:
    """Ölçümün sabitleri; sonuç dizinine yazılır ve sürdürmede aynı olmaları denetlenir."""

    model: str
    effort: str
    embedding: Embedding
    auth: Auth
    profile: ProductProfile
    max_budget_usd: float
    timeout_s: int
    agent: AgentName
    agent_bin: Path
    codex_home: Path | None
    ccm_bin_dir: Path
    ccm_model_dir: Path
    corpus_dir: Path
    out_dir: Path
    # Depo başına bir kez kurulan indekslerin yeri (repo dışında); koşular kopyasını alır.
    index_cache_dir: Path


@dataclass(frozen=True)
class Secrets:
    """Ortamdan okunan kimlik bilgileri; loglanmaz ve kayıtlara girmez.

    Ajana `--auth` ile seçilen tek bir Claude kimliği geçer. OpenAI seçildiğinde anahtar,
    koşu süresince 0600 izinli MCP yapılandırmasında durur ve koşu bitince çalışma diziniyle
    birlikte silinir.
    """

    claude_oauth_token: str | None
    anthropic_api_key: str | None
    openai_api_key: str | None


@dataclass(frozen=True)
class Usage:
    """Claude Code'un sonuç olayındaki toplam token kullanımı."""

    input_tokens: int
    output_tokens: int
    cache_creation_input_tokens: int
    cache_read_input_tokens: int


@dataclass(frozen=True)
class Transcript:
    """stream-json dökümünden çıkarılan ölçüler."""

    result_text: str
    subtype: str
    is_error: bool
    # Claude Code'un API hatası HTTP durumu (`api_error_status`); hata yoksa ve Codex'te None.
    api_error_status: int | None
    # Codex `exec` tur sayısı ve dolar maliyeti bildirmez: o ajanda ikisi de None'dır.
    num_turns: int | None
    duration_ms: int
    cost_usd: float | None
    usage: Usage
    tool_calls: dict[str, int]
    mcp_status: str
    api_key_source: str
    tools: tuple[str, ...]


@dataclass(frozen=True)
class Init:
    """`init` olayından yalıtım denetimi için gerekenler."""

    mcp_status: str
    api_key_source: str
    tools: tuple[str, ...]


@dataclass(frozen=True)
class RunRecord:
    """Bir koşunun kaydı; `runs/<run_id>.json` olarak yazılır.

    Sürüm 3: `hook` H kolunda hook kararlarının sayısıdır (`{"deny": n, "pass_": m}`), diğer
    kollarda `null`. Sürüm 2: `index_copy_s` önceden kurulmuş indeksin çalışma kopyasına
    kopyalanma süresi,
    `index_build_s` o indeksin bir kez kurulma süresidir (A kolunda ikisi de `null`). Sürüm 1
    kayıtlarında `record_version` yoktur ve `index_s` her koşuda yapılan tam indekslemedir.
    """

    record_version: int
    run_id: str
    task_id: str
    repo: str
    category: str
    arm: Arm
    rep: int
    model: str
    effort: str
    embedding: Embedding
    index_copy_s: float | None
    index_build_s: float | None
    wall_s: float | None
    transcript: Transcript | None
    score: Score | None
    failure: str | None
    hook: HookCounts | None


@dataclass(frozen=True)
class HookCounts:
    """H kolunda hook'un kararları: kaç Grep paketle karşılandı, kaçı geçti."""

    deny: int
    pass_: int


@dataclass(frozen=True)
class RunPaths:
    """Bir koşunun sonuç dizinindeki dosyaları."""

    transcript: Path
    stderr: Path
    diff: Path
    record: Path
    hook_log: Path


@dataclass(frozen=True)
class IndexInputs:
    """Önbellekteki indeksi belirleyen girdiler; anahtar ve `entry.json` bunlardan türer."""

    repo: str
    corpus_commit: str
    embedding: Embedding
    # local: model dizini; openai: OpenAI model adı.
    embedding_model: str
    ccm_cli_sha256: str


@dataclass(frozen=True)
class EmbeddingIdentity:
    """Etkin indeks neslinin `ccm_manifest.json` dosyasındaki `embedding` alanı."""

    provider: str
    model: str
    revision: str | None
    dim: int


@dataclass(frozen=True)
class IndexCacheEntry:
    """`entry.json`: girdiler, tek seferlik kurulum süresi ve kurulan vektörlerin kimliği."""

    schema_version: int
    inputs: IndexInputs
    build_s: float
    vectors: EmbeddingIdentity


@dataclass(frozen=True)
class CachedIndex:
    """Doğrulanmış önbellek girdisi: kopyalanacak `.git/ccm-bench` dizini ve kaydı."""

    index_dir: Path
    entry: IndexCacheEntry


@dataclass(frozen=True)
class IndexBuild:
    """Bir `ccm-cli index` çalıştırmasının süresi ve stderr'i (uyarılar dahil)."""

    build_s: float
    stderr: str


def run_id(task: Task, arm: Arm, rep: int) -> str:
    """Koşunun dosya adlarında kullanılan kimliği."""
    return f"{task.id}__{arm}__{rep}"


def record_path(out_dir: Path, name: str) -> Path:
    """Koşu kaydının yolu; varlığı koşunun tamamlandığını gösterir."""
    return out_dir / "runs" / f"{name}.json"


def run_paths(out_dir: Path, name: str) -> RunPaths:
    """Koşunun döküm, stderr, diff ve kayıt yolları."""
    transcript = out_dir / "transcripts" / f"{name}.jsonl"
    return RunPaths(
        transcript=transcript,
        stderr=transcript.with_suffix(".stderr"),
        diff=out_dir / "diffs" / f"{name}.diff",
        record=record_path(out_dir, name),
        hook_log=transcript.with_suffix(".hook.jsonl"),
    )


def disposition(error: RunError) -> Disposition:
    """Koşu hatasının akıbeti: ajanın sonucu kaydedilir, altyapı hatası harness'ı durdurur."""
    if isinstance(error, SetupError):
        return "stop"
    if isinstance(error, AgentTimeout | TranscriptError):
        return "record"
    raise TypeError(f"no disposition is defined for {type(error).__name__}: {error}")


def write_atomic(path: Path, text: str) -> None:
    """Dosyayı aynı dizinde benzersiz adlı geçici dosyadan atomik yazar.

    Yarım dosya hiçbir zaman tamamlanmış sayılmaz; eşzamanlı yazıcılar geçici dosyada çakışmaz.
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(
        dir=path.parent, prefix=f".{path.name}.", suffix=".tmp"
    )
    staged = Path(temporary)
    try:
        with os.fdopen(descriptor, "w") as handle:
            handle.write(text)
        staged.replace(path)
    finally:
        # Yazma ya da taşıma başarısızsa geçici dosya kalmasın; hata yine yükselir.
        staged.unlink(missing_ok=True)


def file_sha256(path: Path) -> str:
    """Bir dosyanın SHA-256 özeti (parça parça okunur)."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def mcp_tool(name: str) -> str:
    """CCM aracının Claude Code'daki adı."""
    return f"mcp__{SERVER}__{name}"


def granted_tools(profile: ProductProfile, arm: Arm) -> tuple[str, ...]:
    """Ürün profili ve kola göre ajana açılan CCM araçları."""
    if arm == "A":
        return ()
    if arm in ("F", "H") and profile != "context-first-pilot":
        raise SetupError(f"arm {arm} exists only in the context-first-pilot profile")
    if arm in ("D", "E") and profile != "preregistered":
        raise SetupError(f"arm {arm} exists only in the preregistered profile")
    if profile == "context-first-pilot":
        return CONTEXT_DEFAULT_TOOLS if arm in ("B", "F", "H") else LEGACY_CCM_TOOLS
    if arm in ("D", "E"):
        return CONTEXT_DEFAULT_TOOLS
    return LEGACY_CCM_TOOLS if arm == "B" else ("search_code",)


def uses_legacy_discovery(profile: ProductProfile, arm: Arm) -> bool:
    """Bu kol eski yapısal araçların tools/list içinde görünmesini gerektiriyor mu."""
    if arm == "A":
        return False
    if arm in ("D", "E", "F", "H"):
        return False
    return profile == "preregistered" or arm == "C"


def arm_note(profile: ProductProfile, arm: Arm) -> str:
    """Kolun ürün yönlendirme notu; baseline not eklemez."""
    if arm == "A":
        return ""
    if arm in ("F", "H") and profile != "context-first-pilot":
        raise SetupError(f"arm {arm} exists only in the context-first-pilot profile")
    if arm in ("D", "E") and profile != "preregistered":
        raise SetupError(f"arm {arm} exists only in the preregistered profile")
    if arm in ("D", "E"):
        return CONTEXT_NOTE
    if profile == "context-first-pilot":
        if arm == "F":
            return CONTEXT_NOTE + FORCED_NOTE
        return CONTEXT_NOTE if arm in ("B", "H") else GRAPH_NOTE
    return GRAPH_NOTE if arm == "B" else SEARCH_NOTE


def expected_tools(profile: ProductProfile, arm: Arm, connected: bool) -> tuple[str, ...]:
    """Ajanın görmesi gereken araç kümesi; MCP bağlanmadıysa yalnız yerleşik araçlar."""
    granted = granted_tools(profile, arm) if connected else ()
    return tuple(sorted((*BUILTIN_TOOLS, *(mcp_tool(tool) for tool in granted))))


def verify_isolation(transcript: Transcript, profile: ProductProfile, arm: Arm, auth: Auth) -> None:
    """Kimlik kaynağı ya da araç kümesi deney tanımına uymuyorsa ölçümü durdurur."""
    expected_source = AUTH_SOURCE[auth]
    if transcript.api_key_source != expected_source:
        raise IsolationError(
            f"Claude Code reported authentication source {transcript.api_key_source!r}, "
            f"expected {expected_source!r} for --auth {auth}; stopping before more runs use "
            "the wrong account"
        )
    expected = expected_tools(profile, arm, transcript.mcp_status == "connected")
    if transcript.tools != expected:
        raise IsolationError(
            f"arm {arm} exposed tools {list(transcript.tools)}, expected {list(expected)}"
        )


def hit_quota(transcript: Transcript) -> bool:
    """Sonuç bir kullanım ya da hız sınırı veya aşırı yük hatası mı."""
    text = transcript.result_text.lower()
    return transcript.is_error and (
        transcript.api_error_status == QUOTA_HTTP_STATUS
        or any(marker in text for marker in QUOTA_MARKERS)
    )


def claude_rejection(lines: list[str]) -> float | None:
    """Claude dökümünde reddedilmiş sınır olayının sıfırlanma anı; olay yoksa None.

    Claude Code sınırdaki isteği `rate_limit_event` ile `status: rejected` olarak bildirir;
    bazen ardından sonuç olayı gelmez, bu yüzden sonuç olayından önce bakılır.
    """
    for number, line in enumerate(lines, 1):
        if '"rate_limit_event"' not in line:
            continue
        event = as_object(json_line(line, number), f"transcript line {number}", TranscriptError)
        info = as_object(event.get("rate_limit_info"), "rate_limit_info", TranscriptError)
        if info.get("status") == "rejected":
            return float(as_int(info.get("resetsAt"), "rate_limit_info.resetsAt", TranscriptError))
    return None


def codex_quota_reset(text: str) -> float | None:
    """Codex sınır mesajındaki "try again at …" anı (yerel saat); yoksa None."""
    match = CODEX_RETRY_AT.search(text)
    if match is None:
        return None
    moment = datetime.strptime(
        f"{match['month']} {match['day']} {match['year']} {match['clock']}", "%b %d %Y %I:%M %p"
    )
    return moment.timestamp()


def hit_auth_failure(transcript: Transcript) -> bool:
    """Sonuç bir kimlik doğrulama hatası mı (geçersiz, eksik ya da iptal edilmiş jeton)."""
    text = transcript.result_text.lower()
    return transcript.is_error and any(marker in text for marker in AUTH_MARKERS)


def memory_files_above(workspace: Path) -> list[Path]:
    """Claude Code'un üst dizinlerden yükleyeceği CLAUDE.md dosyaları; boş olmalıdır."""
    return [
        parent / name
        for parent in workspace.parents
        for name in ("CLAUDE.md", "CLAUDE.local.md")
        if (parent / name).is_file()
    ]


def prepare_workspace(corpus: Path, workspace: Path) -> None:
    """Korpusu temiz, tek commit'lik bir git deposuna çıkarır."""
    export_corpus(corpus, workspace)
    init_git(workspace, "main")


def index_db_path(workspace: Path) -> Path:
    """CCM artifact'lerini ajanın kaynak keşfinden gizli, proje-içi bir yolda tutar."""
    return workspace / ".git" / "ccm-bench" / "ccm_db"


def ccm_env(settings: Settings, home: Path, secrets: Secrets) -> dict[str, str]:
    """`ccm-cli` ve `ccm-mcp` ortamı: geçici HOME ve seçilen embedding kaynağı."""
    base = {"PATH": SYSTEM_PATH, "HOME": str(home), "RUST_LOG": "warn"}
    if settings.embedding == "local":
        return {**base, "CCM_MODEL_DIR": str(settings.ccm_model_dir)}
    if secrets.openai_api_key is None:
        raise SetupError("--embedding openai needs OPENAI_API_KEY in the environment")
    return {
        **base,
        "EMBEDDING_PROVIDER": "openai",
        "EMBEDDING_MODEL": OPENAI_EMBEDDING_MODEL,
        "OPENAI_API_KEY": secrets.openai_api_key,
    }


def prepare_run_workspace(corpus: Path, workspace: Path) -> None:
    """`prepare_workspace`; git ya da tar hatası altyapı hatası olarak yükselir."""
    try:
        prepare_workspace(corpus, workspace)
    except WorkspaceError as error:
        raise SetupError(f"could not prepare {workspace} from {corpus}: {error}") from error


def index_workspace(settings: Settings, workspace: Path, env: dict[str, str]) -> IndexBuild:
    """Ajan başlamadan önce tam indeks kurar (kullanıcının indeksi hazır bulduğu durum).

    Çıkış kodu 0 vektörlerin kurulduğunu göstermez: embedding kaynağına ulaşılamazsa
    `ccm-cli` yalnız uyarı yazıp graf-yalnız indeksle 0 döner; bunu `read_vectors` denetler.
    """
    started = time.monotonic()
    completed = subprocess.run(
        [
            str(settings.ccm_bin_dir / "ccm-cli"),
            "index",
            "--path",
            str(workspace),
            "--db-path",
            str(index_db_path(workspace)),
        ],
        cwd=workspace,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise IndexCacheError(
            f"ccm-cli index failed for {workspace} (exit {completed.returncode}): "
            f"{completed.stderr[-2000:]}"
        )
    return IndexBuild(build_s=time.monotonic() - started, stderr=completed.stderr)


def embedding_model_name(settings: Settings) -> str:
    """Vektörleri belirleyen model: yerelde model dizini, OpenAI'da model adı."""
    if settings.embedding == "local":
        return str(settings.ccm_model_dir)
    return OPENAI_EMBEDDING_MODEL


def corpus_commit(corpus_repo: Path) -> str:
    """Korpus klonunun HEAD commit'i; dışa aktarılan ağaç buna bağlıdır."""
    try:
        return run_git(corpus_repo, ("rev-parse", "HEAD")).strip()
    except WorkspaceError as error:
        raise IndexCacheError(f"cannot read the corpus HEAD of {corpus_repo}: {error}") from error


def index_inputs(settings: Settings, repo: str) -> IndexInputs:
    """Bu koşunun kullanacağı indeksin girdileri (korpus commit'i ve `ccm-cli` özeti okunur)."""
    return IndexInputs(
        repo=repo,
        corpus_commit=corpus_commit(settings.corpus_dir / repo),
        embedding=settings.embedding,
        embedding_model=embedding_model_name(settings),
        ccm_cli_sha256=file_sha256(settings.ccm_bin_dir / "ccm-cli"),
    )


def index_cache_key(inputs: IndexInputs) -> str:
    """Önbellek anahtarı: okunur önek ve tüm girdilerin özeti.

    Girdilerden biri (korpus commit'i, embedding kaynağı ya da modeli, `ccm-cli`) değişirse
    anahtar da değişir; eski girdi yeniden kullanılmaz.
    """
    canonical = json.dumps(asdict(inputs), sort_keys=True)
    digest = hashlib.sha256(canonical.encode()).hexdigest()[:16]
    return f"{inputs.repo}-{inputs.embedding}-{inputs.corpus_commit[:12]}-{digest}"


def cached_index_dir(root: Path) -> Path:
    """Önbellek girdisinde kopyalanacak CCM artifact dizini."""
    return root / "ws" / ".git" / "ccm-bench"


def entry_json(entry: IndexCacheEntry) -> str:
    """`entry.json` içeriği."""
    return json.dumps(asdict(entry), indent=2, sort_keys=True) + "\n"


def parse_identity(value: JsonValue, context: str) -> EmbeddingIdentity:
    """Manifestteki ya da `entry.json`daki embedding kimliğini okur."""
    data = as_object(value, context, IndexCacheError)
    revision = data.get("revision")
    return EmbeddingIdentity(
        provider=as_str(data.get("provider"), f"{context}.provider", IndexCacheError),
        model=as_str(data.get("model"), f"{context}.model", IndexCacheError),
        revision=(
            None if revision is None else as_str(revision, f"{context}.revision", IndexCacheError)
        ),
        dim=as_int(data.get("dim"), f"{context}.dim", IndexCacheError),
    )


def parse_inputs(value: JsonValue, context: str) -> IndexInputs:
    """`entry.json` içindeki girdileri okur."""
    data = as_object(value, context, IndexCacheError)
    embedding = as_str(data.get("embedding"), f"{context}.embedding", IndexCacheError)
    if embedding not in ("local", "openai"):
        raise IndexCacheError(f"{context}.embedding: unknown embedding kind {embedding!r}")
    return IndexInputs(
        repo=as_str(data.get("repo"), f"{context}.repo", IndexCacheError),
        corpus_commit=as_str(
            data.get("corpus_commit"), f"{context}.corpus_commit", IndexCacheError
        ),
        embedding="local" if embedding == "local" else "openai",
        embedding_model=as_str(
            data.get("embedding_model"), f"{context}.embedding_model", IndexCacheError
        ),
        ccm_cli_sha256=as_str(
            data.get("ccm_cli_sha256"), f"{context}.ccm_cli_sha256", IndexCacheError
        ),
    )


def parse_entry(value: JsonValue, path: Path) -> IndexCacheEntry:
    """`entry.json` içeriğini okur; bilinmeyen şema sürümü açık hatadır."""
    data = as_object(value, str(path), IndexCacheError)
    version = as_int(data.get("schema_version"), f"{path}: schema_version", IndexCacheError)
    if version != INDEX_CACHE_SCHEMA:
        raise IndexCacheError(
            f"{path}: index cache schema {version}, expected {INDEX_CACHE_SCHEMA}; "
            "use another --index-cache-dir"
        )
    return IndexCacheEntry(
        schema_version=version,
        inputs=parse_inputs(data.get("inputs"), f"{path}: inputs"),
        build_s=as_float(data.get("build_s"), f"{path}: build_s", IndexCacheError),
        vectors=parse_identity(data.get("vectors"), f"{path}: vectors"),
    )


def require_vectors(
    identity: EmbeddingIdentity | None, embedding: Embedding, manifest: Path, stderr: str
) -> EmbeddingIdentity:
    """Manifestin seçilen kaynaktan vektör kaydettiğini doğrular.

    `None` kimlik graf-yalnız indekstir (embedding kaynağına ulaşılamadı ya da kapalıydı);
    o indeksle B/C kolları anlamsal aramasız çalışırdı, bu yüzden önbellek hazır sayılmaz.
    """
    if identity is None:
        raise IndexCacheError(
            f"{manifest} records no embedding: ccm-cli built a graph-only index although "
            f"--embedding {embedding} was requested; ccm-cli stderr: {stderr[-1500:]}"
        )
    if identity.provider != embedding:
        raise IndexCacheError(
            f"{manifest} records embedding provider {identity.provider!r}, expected {embedding!r}"
        )
    if embedding == "openai" and identity.model != OPENAI_EMBEDDING_MODEL:
        raise IndexCacheError(
            f"{manifest} records OpenAI model {identity.model!r}, "
            f"expected {OPENAI_EMBEDDING_MODEL!r}"
        )
    return identity


def active_manifest(index_dir: Path) -> Path:
    """Etkin indeks neslinin manifesti: `ccm_current` → `.ccm-generations/<id>/`."""
    pointer = index_dir / "ccm_current"
    if not pointer.is_file():
        raise IndexCacheError(f"{pointer} is missing; ccm-cli did not activate an index")
    generation = pointer.read_text().strip()
    manifest = index_dir / ".ccm-generations" / generation / "ccm_manifest.json"
    if not manifest.is_file():
        raise IndexCacheError(
            f"{pointer} names generation {generation!r} but {manifest} is missing"
        )
    return manifest


def read_vectors(index_dir: Path, embedding: Embedding, stderr: str) -> EmbeddingIdentity:
    """Etkin manifestin `embedding` alanını okur ve vektörlerin kurulduğunu doğrular."""
    manifest = active_manifest(index_dir)
    try:
        data: JsonValue = json.loads(manifest.read_text())
    except json.JSONDecodeError as error:
        raise IndexCacheError(f"{manifest} is not valid JSON") from error
    value = as_object(data, str(manifest), IndexCacheError).get("embedding")
    identity = None if value is None else parse_identity(value, f"{manifest}: embedding")
    return require_vectors(identity, embedding, manifest, stderr)


def build_index(settings: Settings, inputs: IndexInputs, root: Path, env: dict[str, str]) -> None:
    """Girdiyi geçici bir dizinde kurar, doğrular ve atomik olarak `root` adına taşır.

    `root` yalnız `entry.json` dahil eksiksizse var olur; yarım kurulum hiçbir zaman hazır
    görünmez. Başarısız kurulumun geçici dizini silinir ve hata yükselir.
    """
    staging = Path(tempfile.mkdtemp(dir=root.parent, prefix=f".{root.name}.building-"))
    try:
        workspace = staging / "ws"
        prepare_run_workspace(settings.corpus_dir / inputs.repo, workspace)
        build = index_workspace(settings, workspace, env)
        vectors = read_vectors(index_db_path(workspace).parent, inputs.embedding, build.stderr)
        entry = IndexCacheEntry(
            schema_version=INDEX_CACHE_SCHEMA,
            inputs=inputs,
            build_s=build.build_s,
            vectors=vectors,
        )
        (staging / "entry.json").write_text(entry_json(entry))
        staging.rename(root)
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def read_entry(root: Path) -> IndexCacheEntry:
    """Var olan önbellek girdisinin `entry.json` kaydı."""
    path = root / "entry.json"
    if not path.is_file():
        raise IndexCacheError(f"{root} exists without entry.json; it was not built by this harness")
    try:
        raw: JsonValue = json.loads(path.read_text())
    except json.JSONDecodeError as error:
        raise IndexCacheError(f"{path} is not valid JSON") from error
    return parse_entry(raw, path)


def verify_entry(entry: IndexCacheEntry, inputs: IndexInputs, root: Path) -> None:
    """Kayıtlı girdiler bu koşununkilerle aynı olmalı; fark sessiz yeniden kurulum değil hatadır."""
    if entry.inputs != inputs:
        raise IndexCacheError(
            f"{root / 'entry.json'} was built from {asdict(entry.inputs)}, but this run needs "
            f"{asdict(inputs)}; remove that cache entry or use another --index-cache-dir"
        )


def cached_index(settings: Settings, repo: str, env: dict[str, str]) -> CachedIndex:
    """Deponun indeksini bir kez, temiz bir dışa aktarımda kurar; doğrulanmış girdiyi döner.

    Kaynaklar her koşuda aynı dışa aktarımdan geldiği için kopyalanan indeks çalışma
    kopyasıyla birebir eşleşir; auto-refresh onu taze bulur. Eşzamanlı harness süreçleri
    aynı önbelleği kilitle bekler. Her kullanımda `entry.json` ve etkin manifestteki vektör
    kimliği yeniden denetlenir.
    """
    inputs = index_inputs(settings, repo)
    root = settings.index_cache_dir / index_cache_key(inputs)
    settings.index_cache_dir.mkdir(parents=True, exist_ok=True)
    with (settings.index_cache_dir / f"{root.name}.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if not root.exists():
            build_index(settings, inputs, root, env)
        entry = read_entry(root)
        verify_entry(entry, inputs, root)
        vectors = read_vectors(cached_index_dir(root), inputs.embedding, "(cached index)")
        if vectors != entry.vectors:
            raise IndexCacheError(
                f"{root}: the active manifest records {vectors}, entry.json records "
                f"{entry.vectors}; the cache entry was modified after it was built"
            )
    return CachedIndex(index_dir=cached_index_dir(root), entry=entry)


def copy_index(cache: Path, workspace: Path) -> float:
    """Önbellekteki indeksi çalışma kopyasına alır; geçen süreyi döndürür."""
    started = time.monotonic()
    shutil.copytree(cache, workspace / ".git" / "ccm-bench", symlinks=True)
    return time.monotonic() - started


def ccm_server_env(workspace: Path, env: dict[str, str], expose_legacy: bool) -> dict[str, str]:
    """CCM MCP ortamı: tek kök, repo içi indeks ve gerektiğinde legacy discovery."""
    server_env = {
        **env,
        "CCM_PROJECT_ROOT": str(workspace),
        "CCM_ALLOWED_ROOTS": str(workspace),
        "CCM_DB_PATH": str(index_db_path(workspace)),
    }
    if expose_legacy:
        server_env["CCM_MCP_LEGACY_TOOLS"] = "1"
    return server_env


def mcp_config(settings: Settings, server_env: dict[str, str]) -> dict[str, JsonValue]:
    """Claude Code'un `--mcp-config` dosyasının içeriği: yalnız CCM sunucusu."""
    environment: dict[str, JsonValue] = {key: value for key, value in server_env.items()}
    server: dict[str, JsonValue] = {
        "type": "stdio",
        "command": str(settings.ccm_bin_dir / "ccm-mcp"),
        "args": [],
        "env": environment,
    }
    return {"mcpServers": {SERVER: server}}


def write_private(path: Path, text: str) -> None:
    """Dosyayı yalnız sahibinin okuyabileceği izinlerle yazar."""
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w") as handle:
        handle.write(text)


def claude_command(
    settings: Settings, task: Task, arm: Arm, config_path: Path, hooks_path: Path | None
) -> list[str]:
    """Kolun `claude -p` komutu; kollar yalnız MCP araçları ve proje notunda ayrışır."""
    command = [
        str(settings.agent_bin),
        "-p",
        prompt_for(task),
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        settings.model,
        "--effort",
        settings.effort,
        "--max-budget-usd",
        f"{settings.max_budget_usd:.2f}",
        "--no-session-persistence",
        "--permission-mode",
        "dontAsk",
        "--permission-prompts",
        "none",
        "--strict-mcp-config",
        "--tools",
        ",".join(BUILTIN_TOOLS),
    ]
    if arm == "A":
        return [*command, "--allowedTools", ",".join(BUILTIN_TOOLS)]
    granted = granted_tools(settings.profile, arm)
    denied = [mcp_tool(tool) for tool in ALL_CCM_TOOLS if tool not in granted]
    command += [
        "--mcp-config",
        str(config_path),
        "--append-system-prompt",
        arm_note(settings.profile, arm),
        "--allowedTools",
        ",".join([*BUILTIN_TOOLS, *(mcp_tool(tool) for tool in granted)]),
    ]
    if hooks_path is not None:
        command += ["--settings", str(hooks_path)]
    return [*command, "--disallowedTools", ",".join(denied)] if denied else command


def claude_credentials(auth: Auth, secrets: Secrets) -> dict[str, str]:
    """Seçilen kimlik kipinin ajana geçen tek ortam değişkeni."""
    if auth == "subscription":
        if secrets.claude_oauth_token is None:
            raise SetupError(
                "--auth subscription needs CLAUDE_CODE_OAUTH_TOKEN (claude setup-token)"
            )
        return {"CLAUDE_CODE_OAUTH_TOKEN": secrets.claude_oauth_token}
    if secrets.anthropic_api_key is None:
        raise SetupError("--auth api-key needs ANTHROPIC_API_KEY")
    return {"ANTHROPIC_API_KEY": secrets.anthropic_api_key}


def claude_env(home: Path, auth: Auth, secrets: Secrets) -> dict[str, str]:
    """Ajan ortamı: geçici HOME ve CLAUDE_CONFIG_DIR, yalnız seçilen Claude kimliği."""
    return {
        "PATH": SYSTEM_PATH,
        "HOME": str(home),
        "CLAUDE_CONFIG_DIR": str(home / ".claude"),
        **claude_credentials(auth, secrets),
        "LANG": "en_US.UTF-8",
        "DISABLE_AUTOUPDATER": "1",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    }


def stop_group(pid: int) -> None:
    """Sürecin oturum grubunu (MCP sunucusu gibi alt süreçlerle) sonlandırır."""
    try:
        os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        # Grup zaten kapanmış: sonlandırılacak süreç kalmamış.
        return


def run_agent(
    command: list[str],
    workspace: Path,
    env: dict[str, str],
    timeout_s: int,
    transcript: Path,
    stderr: Path,
) -> float:
    """Ajanı çalıştırır, dökümü dosyaya yazar ve duvar saatini döner.

    stdin kapalıdır: Codex boru olan stdin'i isteme ekler, Claude Code da okuyabilir. Ajan
    ayrı oturumda çalıştığı için Ctrl-C ona ulaşmaz: zaman aşımı, KeyboardInterrupt ya da
    (harness'ın istisnaya çevirdiği) SIGTERM'de süreç grubu durdurulur, istisna aynen yükselir.
    """
    transcript.parent.mkdir(parents=True, exist_ok=True)
    stderr.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    with transcript.open("w") as out, stderr.open("w") as err:
        process = subprocess.Popen(
            command,
            cwd=workspace,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=out,
            stderr=err,
            start_new_session=True,
        )
        try:
            process.wait(timeout=timeout_s)
        except subprocess.TimeoutExpired as error:
            raise AgentTimeout(
                f"the agent did not finish within {timeout_s} s; see {transcript}"
            ) from error
        finally:
            # Normal bitişte MCP sunucusu gibi artakalanları, diğer durumlarda ajanı da durdurur.
            stop_group(process.pid)
            process.wait()
    return time.monotonic() - started


def json_line(line: str, number: int) -> JsonValue:
    """Döküm satırını JSON olarak okur."""
    try:
        value: JsonValue = json.loads(line)
    except json.JSONDecodeError as error:
        raise TranscriptError(f"transcript line {number} is not JSON: {line[:200]}") from error
    return value


def server_status(event: dict[str, JsonValue]) -> str:
    """`init` olayındaki CCM sunucusunun durumu; sunucu yoksa `none`."""
    servers = event.get("mcp_servers")
    if servers is None:
        return "none"
    for server in as_list(servers, "init.mcp_servers", TranscriptError):
        entry = as_object(server, "init.mcp_servers[]", TranscriptError)
        if entry.get("name") == SERVER:
            return as_str(entry.get("status"), "init.mcp_servers[].status", TranscriptError)
    return "none"


def init_from(event: dict[str, JsonValue]) -> Init:
    """`init` olayını okur; kimlik kaynağı bildirilmemişse `missing` olur ve denetim durdurur."""
    source = event.get("apiKeySource")
    tools = as_list(event.get("tools"), "init.tools", TranscriptError)
    return Init(
        mcp_status=server_status(event),
        api_key_source=(
            "missing" if source is None else as_str(source, "init.apiKeySource", TranscriptError)
        ),
        tools=tuple(sorted(as_str(tool, "init.tools[]", TranscriptError) for tool in tools)),
    )


def tool_uses(event: dict[str, JsonValue]) -> list[str]:
    """Asistan mesajındaki araç çağrılarının adları."""
    message = as_object(event.get("message"), "assistant.message", TranscriptError)
    content = message.get("content")
    if not isinstance(content, list):
        return []
    names: list[str] = []
    for block in content:
        entry = as_object(block, "assistant.message.content[]", TranscriptError)
        if entry.get("type") == "tool_use":
            names.append(as_str(entry.get("name"), "tool_use.name", TranscriptError))
    return names


def transcript_from(
    result: dict[str, JsonValue], tool_calls: dict[str, int], init: Init
) -> Transcript:
    """Sonuç olayını ölçülere çevirir; eksik alan biçim hatasıdır."""
    usage = as_object(result.get("usage"), "result.usage", TranscriptError)
    text = result.get("result")
    status = result.get("api_error_status")
    return Transcript(
        result_text="" if text is None else as_str(text, "result.result", TranscriptError),
        subtype=as_str(result.get("subtype"), "result.subtype", TranscriptError),
        is_error=as_bool(result.get("is_error"), "result.is_error", TranscriptError),
        api_error_status=(
            None if status is None else as_int(status, "result.api_error_status", TranscriptError)
        ),
        num_turns=as_int(result.get("num_turns"), "result.num_turns", TranscriptError),
        duration_ms=as_int(result.get("duration_ms"), "result.duration_ms", TranscriptError),
        cost_usd=as_float(result.get("total_cost_usd"), "result.total_cost_usd", TranscriptError),
        usage=Usage(
            input_tokens=as_int(usage.get("input_tokens"), "usage.input_tokens", TranscriptError),
            output_tokens=as_int(
                usage.get("output_tokens"), "usage.output_tokens", TranscriptError
            ),
            cache_creation_input_tokens=as_int(
                usage.get("cache_creation_input_tokens"),
                "usage.cache_creation_input_tokens",
                TranscriptError,
            ),
            cache_read_input_tokens=as_int(
                usage.get("cache_read_input_tokens"),
                "usage.cache_read_input_tokens",
                TranscriptError,
            ),
        ),
        tool_calls=dict(sorted(tool_calls.items())),
        mcp_status=init.mcp_status,
        api_key_source=init.api_key_source,
        tools=init.tools,
    )


def parse_transcript(lines: list[str]) -> Transcript:
    """stream-json dökümünü okur; `init` ya da sonuç olayı yoksa koşu geçersizdir."""
    tool_calls: dict[str, int] = {}
    init: Init | None = None
    result: dict[str, JsonValue] | None = None
    for number, line in enumerate(lines, 1):
        if not line.strip():
            continue
        event = as_object(json_line(line, number), f"transcript line {number}", TranscriptError)
        kind = event.get("type")
        if kind == "system" and event.get("subtype") == "init":
            init = init_from(event)
        elif kind == "assistant":
            for name in tool_uses(event):
                tool_calls[name] = tool_calls.get(name, 0) + 1
        elif kind == "result":
            result = event
    if init is None:
        raise TranscriptError("the transcript has no init event")
    if result is None:
        raise TranscriptError("the transcript has no result event")
    return transcript_from(result, tool_calls, init)


def write_record(path: Path, record: RunRecord) -> None:
    """Kaydı atomik yazar: yarım dosya hiçbir zaman tamamlanmış koşu sayılmaz."""
    write_atomic(path, json.dumps(asdict(record), indent=2, ensure_ascii=False))


def is_start_event(agent: AgentName, line: str) -> bool:
    """Satır ajanın başlangıç olayı mı (bkz. `START_EVENTS`).

    JSON olmayan satır başlangıç olayı değildir; biçim hatasını sonraki ayrıştırma bildirir.
    """
    try:
        value: JsonValue = json.loads(line)
    except json.JSONDecodeError:
        return False
    if not isinstance(value, dict):
        return False
    if agent == "claude":
        return value.get("type") == "system" and value.get("subtype") == "init"
    return value.get("type") == "thread.started"


def reached_start(agent: AgentName, lines: list[str]) -> bool:
    """Döküm ajanın başladığını gösteriyor mu; göstermiyorsa hata altyapıdadır."""
    return any(is_start_event(agent, line) for line in lines)


def clear_attempt(paths: RunPaths) -> None:
    """Yarıda kalmış önceki denemenin döküm, stderr ve diff dosyalarını siler.

    Böylece yeni kayıtla birlikte duran her dosya bu denemeye aittir.
    """
    for path in (paths.transcript, paths.stderr, paths.diff, paths.hook_log):
        path.unlink(missing_ok=True)


def uses_grep_hook(agent: AgentName, arm: Arm) -> bool:
    """Kolda Claude Code'un PreToolUse hook'u kurulu mu: H, D (amendment 17'den beri) ve E.

    D, ürünün Claude Code için önerilen kurulumudur (`context` + `index_project` + Grep hook).
    E (amendment 20) D'ye Bash'teki `grep`/`rg` aramalarını ekler. Codex'te hook mekanizması
    yoktur. A, B ve C'de hook yoktur.
    """
    return agent == "claude" and arm in ("D", "E", "H")


def hook_matcher(arm: Arm) -> str:
    """Hook'un eşleştiği Claude Code araçları: E'de Grep ve Bash, diğerlerinde yalnız Grep."""
    return "Grep|Bash" if arm == "E" else "Grep"


def hook_settings(settings: Settings, server_env: dict[str, str], hook_log: Path, arm: Arm) -> str:
    """Hook'lu kolların Claude Code ayarı: PreToolUse hook'u (`--settings` dosyası)."""
    environment = {**server_env, "CCM_HOOK_LOG": str(hook_log)}
    assignments = " ".join(f"{key}={shlex.quote(value)}" for key, value in environment.items())
    command = (
        f"env {assignments} {shlex.quote(str(settings.ccm_bin_dir / 'ccm-mcp'))} "
        "hook claude-pre-tool-use"
    )
    hooks: dict[str, JsonValue] = {
        "PreToolUse": [
            {
                "matcher": hook_matcher(arm),
                "hooks": [{"type": "command", "command": command, "timeout": 10}],
            }
        ]
    }
    return json.dumps({"hooks": hooks}, indent=2)


def join_failures(first: str | None, second: str) -> str:
    """İki hata nedenini birleştirir; ilki yoksa ikincisi."""
    return second if first is None else f"{first}; {second}"


def hook_counts(hook_log: Path) -> HookCounts:
    """Hook günlüğündeki kararların sayısı; hook hiç çağrılmadıysa sıfırdır."""
    if not hook_log.is_file():
        return HookCounts(deny=0, pass_=0)
    decisions = [
        as_str(
            as_object(json.loads(line), str(hook_log), TranscriptError).get("decision"),
            f"{hook_log}: decision",
            TranscriptError,
        )
        for line in hook_log.read_text().splitlines()
        if line.strip()
    ]
    return HookCounts(deny=decisions.count("deny"), pass_=decisions.count("pass"))


def marker_contents(workspace: Path, files: tuple[str, ...]) -> dict[str, str | None]:
    """İşaret dosyalarının son hâli; ajanın sildiği ya da taşıdığı dosya `None`dır."""
    return {
        path: (workspace / path).read_text() if (workspace / path).is_file() else None
        for path in files
    }


def stderr_tail(path: Path) -> str:
    """Ajan stderr'inin son kısmı (hata iletisine eklenir)."""
    return path.read_text()[-1500:] if path.is_file() else "(no stderr file)"


def agent_invocation(
    settings: Settings,
    task: Task,
    arm: Arm,
    work: Path,
    workspace: Path,
    home: Path,
    secrets: Secrets,
    server_env: dict[str, str] | None,
    hook_log: Path,
) -> tuple[list[str], dict[str, str]]:
    """Seçilen ajanın komutu ve ortamı; CCM sunucusu yalnız B ve C kollarında verilir."""
    if settings.agent == "claude":
        config_path = work / "mcp.json"
        hooks_path: Path | None = None
        if server_env is not None:
            write_private(config_path, json.dumps(mcp_config(settings, server_env), indent=2))
            if uses_grep_hook(settings.agent, arm):
                hooks_path = work / "hooks.json"
                write_private(hooks_path, hook_settings(settings, server_env, hook_log, arm))
        return claude_command(settings, task, arm, config_path, hooks_path), claude_env(
            home, settings.auth, secrets
        )
    if arm in ("E", "H"):
        raise SetupError(f"arm {arm} (Claude Code hook) is not available for Codex")
    if settings.codex_home is None:
        raise SetupError("--agent codex needs --codex-home")
    server = (
        None
        if server_env is None
        else codex.McpServer(
            name=SERVER,
            command=settings.ccm_bin_dir / "ccm-mcp",
            env=server_env,
            enabled_tools=granted_tools(settings.profile, arm),
            instructions=arm_note(settings.profile, arm),
        )
    )
    try:
        environment = codex.environment(settings.agent_bin, home, settings.codex_home)
    except codex.CodexOutputError as error:
        raise SetupError(str(error)) from error
    return (
        codex.command(
            settings.agent_bin, settings.model, settings.effort, workspace, prompt_for(task), server
        ),
        environment,
    )


def codex_transcript(result: codex.CodexResult, wall_s: float) -> Transcript:
    """Codex sonucunu ortak döküm biçimine çevirir.

    ChatGPT aboneliğinde dolar maliyeti, `exec` kipinde tur sayısı yoktur; kota ölçüsü
    token'lardır. Codex'in `input_tokens` değeri önbellekten okunanları da içerdiği için
    önbelleksiz kısım ayrıştırılır.
    """
    return Transcript(
        result_text=result.result_text if result.failure is None else result.failure,
        subtype="completed" if result.failure is None else "failed",
        is_error=result.failure is not None,
        api_error_status=None,
        num_turns=None,
        duration_ms=round(wall_s * 1000),
        cost_usd=None,
        usage=Usage(
            input_tokens=result.input_tokens - result.cached_input_tokens,
            output_tokens=result.output_tokens,
            cache_creation_input_tokens=0,
            cache_read_input_tokens=result.cached_input_tokens,
        ),
        tool_calls=result.tool_calls,
        mcp_status="unreported",
        api_key_source="chatgpt",
        tools=(),
    )


def verify_codex(transcript: Transcript, profile: ProductProfile, arm: Arm) -> None:
    """Codex kolun dışındaki bir aracı kullandıysa ölçümü durdurur.

    Codex `exec` araç listesini bildirmediği için denetim kullanılan araçlar üzerindendir.
    """
    allowed = {mcp_tool(tool) for tool in granted_tools(profile, arm)}
    used = {name for name in transcript.tool_calls if name.startswith("mcp__")}
    banned = {name for name in transcript.tool_calls if name in CODEX_FORBIDDEN_ITEMS}
    outside = sorted((used - allowed) | banned)
    if outside:
        raise IsolationError(f"arm {arm} used tools outside its set: {outside}")


def read_transcript(settings: Settings, arm: Arm, lines: list[str], wall_s: float) -> Transcript:
    """Ajanın dökümünü ortak biçime çevirir ve yalıtımı doğrular."""
    if settings.agent == "claude":
        transcript = parse_transcript(lines)
        verify_isolation(transcript, settings.profile, arm, settings.auth)
        return transcript
    try:
        result = codex.parse(lines)
    except codex.CodexOutputError as error:
        raise TranscriptError(str(error)) from error
    transcript = codex_transcript(result, wall_s)
    verify_codex(transcript, settings.profile, arm)
    return transcript


def run_one(settings: Settings, task: Task, arm: Arm, rep: int, secrets: Secrets) -> RunRecord:
    """Bir koşuyu yürütür ve kaydeder.

    Ajanın sonucu (zaman aşımı, bozuk döküm, ajan ya da ürün hatası) başarısız kayıt olarak
    saklanır. Altyapı hataları (`SetupError`), kota, kimlik ve yalıtım hataları kayıt
    yazılmadan yükselir; harness durur ve aynı komut koşuyu yeniden dener.
    """
    name = run_id(task, arm, rep)
    paths = run_paths(settings.out_dir, name)
    clear_attempt(paths)
    # Repo ve ev dizini dışında: Claude Code üst dizinlerdeki CLAUDE.md dosyalarını da yükler.
    work = Path(tempfile.mkdtemp(prefix="ccm-l3-"))
    home = work / "home"
    home.mkdir()
    workspace = work / f"ws-{name}"
    memory = memory_files_above(workspace)
    if memory:
        shutil.rmtree(work)
        raise IsolationError(f"Claude Code would load {memory}; set TMPDIR to another directory")
    index_copy_s: float | None = None
    index_build_s: float | None = None
    wall_s: float | None = None
    transcript: Transcript | None = None
    result: Score | None = None
    failure: str | None = None
    agent_started: float | None = None
    try:
        prepare_run_workspace(settings.corpus_dir / task.repo, workspace)
        server_env: dict[str, str] | None = None
        if arm != "A":
            env = ccm_env(settings, home, secrets)
            cache = cached_index(settings, task.repo, env)
            index_build_s = cache.entry.build_s
            index_copy_s = copy_index(cache.index_dir, workspace)
            server_env = ccm_server_env(
                workspace, env, uses_legacy_discovery(settings.profile, arm)
            )
        command, agent_env = agent_invocation(
            settings, task, arm, work, workspace, home, secrets, server_env, paths.hook_log
        )
        agent_started = time.monotonic()
        wall_s = run_agent(
            command, workspace, agent_env, settings.timeout_s, paths.transcript, paths.stderr
        )
        lines = paths.transcript.read_text().splitlines()
        if not reached_start(settings.agent, lines):
            raise AgentStartError(
                f"{name}: the {settings.agent} agent exited before its "
                f"{START_EVENTS[settings.agent]} event; stderr: {stderr_tail(paths.stderr)}"
            )
        rejected_until = claude_rejection(lines) if settings.agent == "claude" else None
        if rejected_until is not None:
            raise QuotaError(
                f"{name}: Claude Code rejected the request at its limit", rejected_until
            )
        transcript = read_transcript(settings, arm, lines, wall_s)
        if hit_auth_failure(transcript):
            raise AuthError(f"{name}: {transcript.result_text[:300]}")
        if hit_quota(transcript):
            raise QuotaError(
                f"{name}: {transcript.result_text[:300]}",
                codex_quota_reset(transcript.result_text) if settings.agent == "codex" else None,
            )
        if transcript.is_error:
            failure = f"{settings.agent} result error ({transcript.subtype})"
        if settings.agent == "claude" and arm != "A" and transcript.mcp_status != "connected":
            mcp_failure = (
                f"CCM MCP server did not connect (status {transcript.mcp_status}); "
                f"see {paths.stderr}"
            )
            failure = mcp_failure if failure is None else f"{failure}; {mcp_failure}"
        contents = {} if task.edit is None else marker_contents(workspace, marker_files(task.edit))
        diff = run_git(workspace, ("diff",))
        if diff:
            paths.diff.parent.mkdir(parents=True, exist_ok=True)
            paths.diff.write_text(diff)
        result = score(task, transcript.result_text, workspace.name, contents)
    except RunError as error:
        if disposition(error) == "stop":
            raise
        if agent_started is not None:
            wall_s = time.monotonic() - agent_started
        failure = str(error)
    finally:
        shutil.rmtree(work)
    # Hook günlüğü okunamazsa koşu yine kaydedilir; hata `failure` alanına eklenir.
    hook: HookCounts | None = None
    hook_failure: str | None = None
    if uses_grep_hook(settings.agent, arm):
        try:
            hook = hook_counts(paths.hook_log)
        except (TranscriptError, json.JSONDecodeError, UnicodeError, OSError) as error:
            hook_failure = f"hook log {paths.hook_log}: {error}"
    record = RunRecord(
        record_version=RECORD_VERSION,
        run_id=name,
        task_id=task.id,
        repo=task.repo,
        category=task.category,
        arm=arm,
        rep=rep,
        model=settings.model,
        effort=settings.effort,
        embedding=settings.embedding,
        index_copy_s=index_copy_s,
        index_build_s=index_build_s,
        wall_s=wall_s,
        transcript=transcript,
        score=result,
        failure=failure if hook_failure is None else join_failures(failure, hook_failure),
        hook=hook,
    )
    write_record(paths.record, record)
    return record
