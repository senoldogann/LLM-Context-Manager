"""L3 görevleri: soru, elle doğrulanmış beklenen cevap ve düzenleme görevlerinin
referans değişikliği.

Beklenen cevaplar CCM kullanılmadan çıkarıldı: Python çağrı yerleri standart `ast` modülüyle,
JavaScript ve Rust çağrı yerleri `rg` çıktısı satır satır okunarak listelendi. Her görevin
`notes` alanı neyin sayıldığını ve neyin bilerek dışarıda bırakıldığını söyler.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

AnswerKind = Literal["symbols", "files"]
Category = Literal["callers", "trap", "files", "types", "locate", "edit"]


@dataclass(frozen=True)
class Item:
    """Beklenen öğe: dosya ve sembol adı; dosya düzeyindeki görevlerde ad boştur.

    `aliases` aynı öğe için kabul edilen diğer adlardır: çağrıyı içeren iç içe fonksiyonun adı
    ya da fonksiyonun ikinci dışa aktarım adı (`res.type` = `res.contentType`).
    """

    path: str
    name: str
    aliases: tuple[str, ...]


@dataclass(frozen=True)
class Replace:
    """Referans düzenleme adımı: dosyada `after` metninden sonraki ilk `old`, `new` olur."""

    file: str
    after: str
    old: str
    new: str


@dataclass(frozen=True)
class Append:
    """Referans düzenleme adımı: metni dosyanın sonuna ekler."""

    file: str
    text: str


@dataclass(frozen=True)
class Marker:
    """Düzenlemeyi gösteren metin; boşluklar yok sayılarak aranır."""

    file: str
    text: str


@dataclass(frozen=True)
class Edit:
    """Ajanın sorudan önce yapacağı değişiklik ve yapıldığını gösteren işaretler.

    `added` yalnız düzenlemeden sonra, `removed` yalnız düzenlemeden önce doğru olan
    öğelerdir: bayat bir cevap `removed` öğelerini listeler ya da `added` öğelerini atlar.
    """

    instructions: str
    steps: tuple[Replace | Append, ...]
    present: tuple[Marker, ...]
    absent: tuple[Marker, ...]
    added: tuple[Item, ...]
    removed: tuple[Item, ...]


@dataclass(frozen=True)
class Task:
    """Bir L3 görevi; `edit` yalnız düzenleme sonrası görevlerde doludur."""

    id: str
    repo: str
    category: Category
    answer_kind: AnswerKind
    question: str
    expected: tuple[Item, ...]
    edit: Edit | None
    notes: str


def sym(path: str, name: str) -> Item:
    """Başka adı olmayan sembol öğesi."""
    return Item(path=path, name=name, aliases=())


def sym_with(path: str, name: str, aliases: tuple[str, ...]) -> Item:
    """Başka adlarla da kabul edilen sembol öğesi."""
    return Item(path=path, name=name, aliases=aliases)


def path_item(path: str) -> Item:
    """Dosya düzeyindeki öğe."""
    return Item(path=path, name="", aliases=())


def symbols(path: str, names: tuple[str, ...]) -> tuple[Item, ...]:
    """Aynı dosyadaki, başka adı olmayan semboller."""
    return tuple(sym(path, name) for name in names)


def label(item: Item) -> str:
    """Öğenin rapordaki yazımı: `yol:ad` ya da yalnız yol."""
    return f"{item.path}:{item.name}" if item.name else item.path


SYMBOL_FORMAT = (
    "Finish your reply with one fenced ```json block that contains only "
    '{"answer": [...]}. Each entry is "<path>:<name>": the file path relative to the '
    "repository root and the function, method or type name (for a method, `method` or "
    "`Class.method`). For a call inside a nested function or closure, give the outermost "
    "named function or method that contains it. List every item once."
)
FILE_FORMAT = (
    "Finish your reply with one fenced ```json block that contains only "
    '{"answer": [...]}. Each entry is a file path relative to the repository root. '
    "List every file once."
)


def prompt_for(task: Task) -> str:
    """Ajana giden istem: (varsa) düzenleme, soru ve cevap biçimi; üç kolda aynıdır."""
    answer_format = SYMBOL_FORMAT if task.answer_kind == "symbols" else FILE_FORMAT
    if task.edit is None:
        return f"{task.question}\n\n{answer_format}"
    return (
        f"First make this change exactly as described.\n\n{task.edit.instructions}\n\n"
        f"Then answer: {task.question}\n\n{answer_format}"
    )


# --- Flask 3.0.3 -------------------------------------------------------------------------------

APP = "src/flask/app.py"
CLI = "src/flask/cli.py"
CTX = "src/flask/ctx.py"
VIEWS = "src/flask/views.py"
BLUEPRINTS = "src/flask/blueprints.py"
SANSIO_APP = "src/flask/sansio/app.py"
SANSIO_BLUEPRINTS = "src/flask/sansio/blueprints.py"
SCAFFOLD = "src/flask/sansio/scaffold.py"

OPTIONS_ROUTE = (
    "    @setupmethod\n"
    "    def options_route(\n"
    "        self, rule: str, **options: t.Any\n"
    "    ) -> t.Callable[[T_route], T_route]:\n"
    '        """Shortcut for :meth:`route` with ``methods=["OPTIONS"]``."""\n'
    '        return self._method_route("OPTIONS", rule, options)\n'
)
DEBUG_FROM_ENV = "def _debug_from_env() -> bool:\n    return get_debug_flag()\n"
SERVER_ERROR_HANDLER = (
    "    def _server_error_handler(\n"
    "        self, error: InternalServerError\n"
    "    ) -> ft.ErrorHandlerCallable | None:\n"
    "        return self._find_error_handler(error, request.blueprints)\n"
)

FLASK_TASKS: tuple[Task, ...] = (
    Task(
        id="flask-callers-ensure-sync",
        repo="flask",
        category="callers",
        answer_kind="symbols",
        question="Which functions or methods in `src/flask/` call `ensure_sync` directly?",
        expected=(
            *symbols(
                APP,
                (
                    "update_template_context",
                    "handle_http_exception",
                    "handle_user_exception",
                    "handle_exception",
                    "dispatch_request",
                    "preprocess_request",
                    "process_response",
                    "do_teardown_request",
                    "do_teardown_appcontext",
                ),
            ),
            sym_with(CTX, "copy_current_request_context", ("wrapper",)),
            sym_with(VIEWS, "as_view", ("view",)),
            sym(VIEWS, "dispatch_request"),
        ),
        edit=None,
        notes=(
            "ast: every call of an attribute or name `ensure_sync` under src/flask, 14 call "
            "sites in 12 functions. copy_current_request_context calls it from the nested "
            "`wrapper`, View.as_view from the nested `view`."
        ),
    ),
    Task(
        id="flask-trap-load-dotenv",
        repo="flask",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions or methods in `src/flask/` call Flask's own `load_dotenv` (the "
            "function defined in `src/flask/cli.py`)? Calls to `dotenv.load_dotenv` from the "
            "python-dotenv package do not count."
        ),
        expected=(sym(APP, "run"), sym(CLI, "_env_file_callback"), sym(CLI, "make_context")),
        edit=None,
        notes=(
            "ast: Flask.run calls cli.load_dotenv; _env_file_callback and "
            "FlaskGroup.make_context call load_dotenv. load_dotenv itself calls "
            "dotenv.load_dotenv twice, the third-party function, which does not count."
        ),
    ),
    Task(
        id="flask-files-globals",
        repo="flask",
        category="files",
        answer_kind="files",
        question="Which files in `src/flask/` import names from the module `src/flask/globals.py`?",
        expected=tuple(
            path_item(f"src/flask/{name}")
            for name in (
                "__init__.py",
                "app.py",
                "blueprints.py",
                "cli.py",
                "ctx.py",
                "debughelpers.py",
                "helpers.py",
                "json/__init__.py",
                "logging.py",
                "templating.py",
                "views.py",
                "wrappers.py",
            )
        ),
        edit=None,
        notes="rg 'from \\.globals import|from \\.\\.globals import' src/flask: 12 files.",
    ),
    Task(
        id="flask-types-scaffold",
        repo="flask",
        category="types",
        answer_kind="symbols",
        question=(
            "Which classes in `src/flask/` inherit, directly or indirectly, from `Scaffold` "
            "(defined in `src/flask/sansio/scaffold.py`)?"
        ),
        expected=(
            sym(SANSIO_APP, "App"),
            sym(SANSIO_BLUEPRINTS, "Blueprint"),
            sym(APP, "Flask"),
            sym(BLUEPRINTS, "Blueprint"),
        ),
        edit=None,
        notes=(
            "class statements: App(Scaffold) and Blueprint(Scaffold) in sansio, Flask(App) and "
            "Blueprint(SansioBlueprint) in the package. Two different classes are named Blueprint."
        ),
    ),
    Task(
        id="flask-locate-app-string",
        repo="flask",
        category="locate",
        answer_kind="symbols",
        question=(
            "Which function in `src/flask/` takes a module and an app name string such as `app` "
            "or `create_app(debug=True)`, decides whether the string names a variable or a "
            "factory call with literal arguments, and returns the application object?"
        ),
        expected=(sym(CLI, "find_app_by_string"),),
        edit=None,
        notes=(
            "find_app_by_string parses the string with ast and reads the attribute or calls the "
            "factory; locate_app only imports the module and delegates to it."
        ),
    ),
    Task(
        id="flask-edit-method-route",
        repo="flask",
        category="edit",
        answer_kind="symbols",
        question=(
            "After the change, which functions or methods in `src/flask/` call `_method_route` "
            "directly?"
        ),
        expected=symbols(SCAFFOLD, ("get", "post", "put", "delete", "options_route")),
        edit=Edit(
            instructions=(
                "In `src/flask/sansio/scaffold.py`:\n\n"
                '1. In `Scaffold.patch`, replace `return self._method_route("PATCH", rule, '
                'options)` with `return self.route(rule, methods=["PATCH"], **options)`.\n'
                "2. Add this method to the `Scaffold` class, directly after `patch`:\n\n"
                f"```python\n{OPTIONS_ROUTE}```"
            ),
            steps=(
                Replace(
                    file=SCAFFOLD,
                    after="    def patch(",
                    old='        return self._method_route("PATCH", rule, options)\n',
                    new='        return self.route(rule, methods=["PATCH"], **options)\n\n'
                    + OPTIONS_ROUTE,
                ),
            ),
            present=(
                Marker(SCAFFOLD, "def options_route("),
                Marker(SCAFFOLD, 'return self._method_route("OPTIONS", rule, options)'),
            ),
            absent=(Marker(SCAFFOLD, '_method_route("PATCH"'),),
            added=(sym(SCAFFOLD, "options_route"),),
            removed=(sym(SCAFFOLD, "patch"),),
        ),
        notes=(
            "Before the change get, post, put, delete and patch call _method_route (ast). The "
            "change moves patch to route() and adds options_route."
        ),
    ),
    Task(
        id="flask-edit-debug-flag",
        repo="flask",
        category="edit",
        answer_kind="symbols",
        question=(
            "After the change, which functions or methods in `src/flask/` call `get_debug_flag` "
            "directly?"
        ),
        expected=(
            sym(APP, "run"),
            sym(CLI, "load_app"),
            sym(CLI, "_debug_from_env"),
            sym(SANSIO_APP, "make_config"),
        ),
        edit=Edit(
            instructions=(
                "In `src/flask/cli.py`:\n\n"
                "1. In `run_command`, replace `debug = get_debug_flag()` with "
                "`debug = _debug_from_env()`.\n"
                "2. Add this function at the end of the file:\n\n"
                f"```python\n{DEBUG_FROM_ENV}```"
            ),
            steps=(
                Replace(
                    file=CLI,
                    after="def run_command(",
                    old="debug = get_debug_flag()",
                    new="debug = _debug_from_env()",
                ),
                Append(file=CLI, text=f"\n\n{DEBUG_FROM_ENV}"),
            ),
            present=(
                Marker(CLI, "def _debug_from_env("),
                Marker(CLI, "debug = _debug_from_env()"),
            ),
            absent=(),
            added=(sym(CLI, "_debug_from_env"),),
            removed=(sym(CLI, "run_command"),),
        ),
        notes=(
            "Before the change Flask.run, ScriptInfo.load_app, run_command and App.make_config "
            "call get_debug_flag (ast). The change routes run_command through _debug_from_env."
        ),
    ),
    Task(
        id="flask-edit-error-handler",
        repo="flask",
        category="edit",
        answer_kind="symbols",
        question=(
            "After the change, which functions or methods in `src/flask/` call "
            "`_find_error_handler` directly?"
        ),
        expected=symbols(
            APP, ("handle_http_exception", "handle_user_exception", "_server_error_handler")
        ),
        edit=Edit(
            instructions=(
                "In `src/flask/app.py`:\n\n"
                "1. In `Flask.handle_exception`, replace `handler = self._find_error_handler("
                "server_error, request.blueprints)` with "
                "`handler = self._server_error_handler(server_error)`.\n"
                "2. Add this method to the `Flask` class, directly after `handle_exception`:\n\n"
                f"```python\n{SERVER_ERROR_HANDLER}```"
            ),
            steps=(
                Replace(
                    file=APP,
                    after="def handle_exception(",
                    old="handler = self._find_error_handler(server_error, request.blueprints)",
                    new="handler = self._server_error_handler(server_error)",
                ),
                Replace(
                    file=APP,
                    after="def handle_exception(",
                    old="    def log_exception(",
                    new=f"{SERVER_ERROR_HANDLER}\n    def log_exception(",
                ),
            ),
            present=(
                Marker(APP, "def _server_error_handler("),
                Marker(APP, "handler = self._server_error_handler(server_error)"),
            ),
            absent=(
                Marker(APP, "handler = self._find_error_handler(server_error, request.blueprints)"),
            ),
            added=(sym(APP, "_server_error_handler"),),
            removed=(sym(APP, "handle_exception"),),
        ),
        notes=(
            "Before the change handle_http_exception, handle_user_exception and handle_exception "
            "call _find_error_handler (ast); the method itself is defined on App in sansio/app.py."
        ),
    ),
)

# --- Express 4.19.2 ----------------------------------------------------------------------------

REQUEST = "lib/request.js"
RESPONSE = "lib/response.js"
ROUTER = "lib/router/index.js"
ROUTE = "lib/router/route.js"
UTILS = "lib/utils.js"

JSON_BODY = (
    "res.jsonBody = function jsonBody(obj) {\n"
    "  var app = this.app;\n"
    "  return stringify(obj, app.get('json replacer'), app.get('json spaces'), "
    "app.get('json escape'));\n"
    "};\n"
)
INVOKE_LAYER = (
    "function invokeLayer(layer, req, res, next) {\n  layer.handle_request(req, res, next);\n}\n"
)
CONTENT_TYPE_OF = "function contentTypeOf(key) {\n  return normalizeType(key).value;\n}\n"

EXPRESS_TASKS: tuple[Task, ...] = (
    Task(
        id="express-trap-res-send",
        repo="express",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions in `lib/` call the response method `res.send` (defined in "
            "`lib/response.js`)? Calls to the `send` package (`send(req, path, options)`) do not "
            "count."
        ),
        expected=(
            sym(RESPONSE, "json"),
            sym(RESPONSE, "jsonp"),
            sym(RESPONSE, "sendStatus"),
            sym_with(RESPONSE, "render", ("done",)),
            sym(ROUTER, "sendOptionsResponse"),
        ),
        edit=None,
        notes=(
            "rg '\\bsend\\(' lib, read per call: res.json, res.jsonp and res.sendStatus call "
            "this.send; res.render calls self.send in its done callback; sendOptionsResponse "
            "calls res.send. res.sendFile and res.sendfile call the send package; the "
            "deprecation strings in res.send are text."
        ),
    ),
    Task(
        id="express-trap-req-get",
        repo="express",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions in `lib/` call the request method `req.get` (also exported as "
            "`req.header`, defined in `lib/request.js`)? Calls to `app.get` or `res.get` do not "
            "count."
        ),
        expected=(
            sym(REQUEST, "range"),
            sym(REQUEST, "protocol"),
            sym(REQUEST, "hostname"),
            sym(REQUEST, "xhr"),
            sym(RESPONSE, "location"),
        ),
        edit=None,
        notes=(
            "rg '\\.get\\(' lib, read per receiver: in request.js `this.get` is req.get (range "
            "and the protocol, hostname and xhr getters), `this.app.get` is app.get and `res.get` "
            "in the fresh getter is res.get; in response.js only res.location calls "
            "this.req.get, every `this.get` there is res.get."
        ),
    ),
    Task(
        id="express-trap-sendfile-helper",
        repo="express",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions in `lib/` call the private helper function `sendfile` declared near "
            "the end of `lib/response.js` (not the methods `res.sendFile` or `res.sendfile`)?"
        ),
        expected=(sym(RESPONSE, "sendFile"), sym(RESPONSE, "sendfile")),
        edit=None,
        notes=(
            "rg '\\bsendfile\\(' lib: the helper is called by res.sendFile and by the deprecated "
            "res.sendfile. The two method names differ only in case."
        ),
    ),
    Task(
        id="express-callers-res-set",
        repo="express",
        category="callers",
        answer_kind="symbols",
        question=(
            "Which functions in `lib/` call the response method `res.set` (also exported as "
            "`res.header`, defined in `lib/response.js`), other than `res.set` itself? Calls to "
            "`app.set` or to Node's `res.setHeader` do not count."
        ),
        expected=(
            *symbols(RESPONSE, ("links", "send", "json", "jsonp")),
            sym_with(RESPONSE, "type", ("contentType",)),
            *symbols(RESPONSE, ("format", "attachment", "append", "location", "redirect")),
            sym(ROUTER, "sendOptionsResponse"),
        ),
        edit=None,
        notes=(
            "rg '\\.set\\(' lib, read per receiver: every this.set in response.js is res.set; "
            "every this.set in application.js is app.set; init.js uses res.setHeader. "
            "res.type is also exported as res.contentType."
        ),
    ),
    Task(
        id="express-locate-trust",
        repo="express",
        category="locate",
        answer_kind="symbols",
        question=(
            "Which function in `lib/` converts the value of the `trust proxy` setting (a "
            "boolean, number, string or function) into the function Express uses to decide "
            "whether to trust a proxy address?"
        ),
        expected=(sym(UTILS, "compileTrust"),),
        edit=None,
        notes="exports.compileTrust in utils.js; app.set calls it when the setting changes.",
    ),
    Task(
        id="express-edit-stringify",
        repo="express",
        category="edit",
        answer_kind="symbols",
        question=(
            "After the change, which functions in `lib/` call the module-level function "
            "`stringify` declared in `lib/response.js`? Calls to `JSON.stringify` do not count."
        ),
        expected=(sym(RESPONSE, "json"), sym(RESPONSE, "jsonBody")),
        edit=Edit(
            instructions=(
                "In `lib/response.js`:\n\n"
                "1. In `res.jsonp`, replace `var body = stringify(val, replacer, spaces, escape)` "
                "with `var body = JSON.stringify(val)`.\n"
                "2. Add this method at the end of the file:\n\n"
                f"```js\n{JSON_BODY}```"
            ),
            steps=(
                Replace(
                    file=RESPONSE,
                    after="res.jsonp = function jsonp(",
                    old="var body = stringify(val, replacer, spaces, escape)",
                    new="var body = JSON.stringify(val)",
                ),
                Append(file=RESPONSE, text=f"\n{JSON_BODY}"),
            ),
            present=(
                Marker(RESPONSE, "res.jsonBody = function jsonBody(obj)"),
                Marker(RESPONSE, "var body = JSON.stringify(val)"),
            ),
            absent=(),
            added=(sym(RESPONSE, "jsonBody"),),
            removed=(sym(RESPONSE, "jsonp"),),
        ),
        notes=(
            "Before the change res.json and res.jsonp call stringify (rg '\\bstringify\\(' lib); "
            "the other matches are JSON.stringify."
        ),
    ),
    Task(
        id="express-edit-handle-request",
        repo="express",
        category="edit",
        answer_kind="symbols",
        question=(
            "After the change, which functions in `lib/` call `Layer.prototype.handle_request` "
            "(defined in `lib/router/layer.js`)?"
        ),
        expected=(
            sym_with(ROUTER, "handle", ("next", "trim_prefix")),
            sym(ROUTE, "invokeLayer"),
        ),
        edit=Edit(
            instructions=(
                "In `lib/router/route.js`:\n\n"
                "1. In `Route.prototype.dispatch`, replace `layer.handle_request(req, res, next);` "
                "with `invokeLayer(layer, req, res, next);`.\n"
                "2. Add this function at the end of the file:\n\n"
                f"```js\n{INVOKE_LAYER}```"
            ),
            steps=(
                Replace(
                    file=ROUTE,
                    after="Route.prototype.dispatch = function dispatch(",
                    old="layer.handle_request(req, res, next);",
                    new="invokeLayer(layer, req, res, next);",
                ),
                Append(file=ROUTE, text=f"\n{INVOKE_LAYER}"),
            ),
            present=(
                Marker(ROUTE, "function invokeLayer(layer, req, res, next)"),
                Marker(ROUTE, "invokeLayer(layer, req, res, next);"),
            ),
            absent=(),
            added=(sym(ROUTE, "invokeLayer"),),
            removed=(sym_with(ROUTE, "dispatch", ("next",)),),
        ),
        notes=(
            "Before the change proto.handle (from its nested next and trim_prefix) and "
            "Route.prototype.dispatch (from its nested next) call handle_request."
        ),
    ),
    Task(
        id="express-edit-normalize-type",
        repo="express",
        category="edit",
        answer_kind="symbols",
        question=(
            "After the change, which functions in `lib/` call `normalizeType` from "
            "`lib/utils.js`? Calls to `normalizeTypes` do not count."
        ),
        expected=(sym(UTILS, "normalizeTypes"), sym(RESPONSE, "contentTypeOf")),
        edit=Edit(
            instructions=(
                "In `lib/response.js`:\n\n"
                "1. In `res.format`, replace `normalizeType(key).value` with "
                "`contentTypeOf(key)`.\n"
                "2. Add this function at the end of the file:\n\n"
                f"```js\n{CONTENT_TYPE_OF}```"
            ),
            steps=(
                Replace(
                    file=RESPONSE,
                    after="res.format = function(obj){",
                    old="normalizeType(key).value",
                    new="contentTypeOf(key)",
                ),
                Append(file=RESPONSE, text=f"\n{CONTENT_TYPE_OF}"),
            ),
            present=(
                Marker(RESPONSE, "function contentTypeOf(key)"),
                Marker(RESPONSE, "this.set('Content-Type', contentTypeOf(key));"),
            ),
            absent=(Marker(RESPONSE, "this.set('Content-Type', normalizeType(key).value);"),),
            added=(sym(RESPONSE, "contentTypeOf"),),
            removed=(sym(RESPONSE, "format"),),
        ),
        notes=(
            "Before the change exports.normalizeTypes (through exports.normalizeType) and "
            "res.format call normalizeType (rg '\\bnormalizeType\\(' lib)."
        ),
    ),
)

# --- serde 1.0.219 -----------------------------------------------------------------------------

DE = "serde_derive/src/de.rs"
SER = "serde_derive/src/ser.rs"
CHECK = "serde_derive/src/internals/check.rs"
PRIVATE_DE = "serde/src/private/de.rs"

WRAP_NEWTYPE_FIELD = (
    "fn wrap_newtype_field(\n"
    "    params: &Parameters,\n"
    "    field_ty: &syn::Type,\n"
    "    serialize_with: &syn::ExprPath,\n"
    "    field_expr: &TokenStream,\n"
    ") -> TokenStream {\n"
    "    wrap_serialize_field_with(params, field_ty, serialize_with, field_expr)\n"
    "}\n"
)
WITH_SERIALIZE_BOUND = (
    "fn with_serialize_bound(\n"
    "    cont: &Container,\n"
    "    generics: &syn::Generics,\n"
    "    filter: fn(&attr::Field, Option<&attr::Variant>) -> bool,\n"
    "    bound: &syn::Path,\n"
    ") -> syn::Generics {\n"
    "    bound::with_bound(cont, generics, filter, bound)\n"
    "}\n"
)
DESERIALIZE_TUPLE_SEQ = (
    "fn deserialize_tuple_seq(\n"
    "    type_path: &TokenStream,\n"
    "    params: &Parameters,\n"
    "    fields: &[Field],\n"
    "    is_struct: bool,\n"
    "    cattrs: &attr::Container,\n"
    "    expecting: &str,\n"
    ") -> Fragment {\n"
    "    deserialize_seq(type_path, params, fields, is_struct, cattrs, expecting)\n"
    "}\n"
)
SERIALIZE_VARIANTS = (
    "serialize_externally_tagged_variant",
    "serialize_internally_tagged_variant",
    "serialize_adjacently_tagged_variant",
    "serialize_untagged_variant",
)

SERDE_TASKS: tuple[Task, ...] = (
    Task(
        id="serde-callers-expr-is-missing",
        repo="serde",
        category="callers",
        answer_kind="symbols",
        question=(
            "Which functions in `serde_derive/src/` call `expr_is_missing` (defined in "
            "`serde_derive/src/de.rs`) directly?"
        ),
        expected=symbols(
            DE,
            (
                "deserialize_seq",
                "deserialize_seq_in_place",
                "deserialize_internally_tagged_variant",
                "deserialize_untagged_variant",
                "deserialize_externally_tagged_newtype_variant",
                "deserialize_map",
                "deserialize_map_in_place",
            ),
        ),
        edit=None,
        notes="rg '\\bexpr_is_missing\\(' serde_derive/src: 8 call sites in 7 functions.",
    ),
    Task(
        id="serde-trap-deserialize-struct",
        repo="serde",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions in `serde_derive/src/` call the function `deserialize_struct` "
            "defined in `serde_derive/src/de.rs`? Only real calls count: code inside `quote!` is "
            "generated output, not a call."
        ),
        expected=symbols(
            DE,
            (
                "deserialize_body",
                "deserialize_externally_tagged_variant",
                "deserialize_internally_tagged_variant",
                "deserialize_untagged_variant",
            ),
        ),
        edit=None,
        notes=(
            "rg '\\bdeserialize_struct\\(' serde_derive/src: 7 matches; 4 are calls, 3 are "
            "`_serde::Deserializer::deserialize_struct` inside quote! (in deserialize_struct, "
            "deserialize_struct_in_place and deserialize_adjacently_tagged_enum)."
        ),
    ),
    Task(
        id="serde-trap-effective-style",
        repo="serde",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions in `serde_derive/src/` call the `effective_style` function defined "
            "in `serde_derive/src/ser.rs`?"
        ),
        expected=symbols(SER, SERIALIZE_VARIANTS),
        edit=None,
        notes=(
            "de.rs and ser.rs each define a private effective_style (skip_deserializing versus "
            "skip_serializing); the two callers in de.rs call de.rs's own function."
        ),
    ),
    Task(
        id="serde-types-deserializer",
        repo="serde",
        category="types",
        answer_kind="symbols",
        question=(
            "Which types in `serde/src/private/de.rs` implement serde's `Deserializer<'de>` trait?"
        ),
        expected=symbols(
            PRIVATE_DE,
            (
                "MissingFieldDeserializer",
                "ContentDeserializer",
                "ContentRefDeserializer",
                "StrDeserializer",
                "BorrowedStrDeserializer",
                "FlatMapDeserializer",
            ),
        ),
        edit=None,
        notes=(
            'rg "Deserializer<\'de> for" serde/src/private/de.rs: 6 impl blocks, some inside '
            "nested modules of the same file."
        ),
    ),
    Task(
        id="serde-locate-tag-conflict",
        repo="serde",
        category="locate",
        answer_kind="symbols",
        question=(
            "Which function in `serde_derive/src/` reports an error when the tag of an "
            "internally tagged enum has the same name as a field of one of its struct variants?"
        ),
        expected=(sym(CHECK, "check_internal_tag_field_name_conflict"),),
        edit=None,
        notes="internals/check.rs; check() calls it for every container.",
    ),
    Task(
        id="serde-edit-wrap-field",
        repo="serde",
        category="edit",
        answer_kind="symbols",
        question=(
            "After the change, which functions in `serde_derive/src/` call "
            "`wrap_serialize_field_with` directly?"
        ),
        expected=symbols(
            SER,
            (
                *SERIALIZE_VARIANTS,
                "serialize_tuple_struct_visitor",
                "serialize_struct_visitor",
                "wrap_newtype_field",
            ),
        ),
        edit=Edit(
            instructions=(
                "In `serde_derive/src/ser.rs`:\n\n"
                "1. In `serialize_newtype_struct`, replace `field_expr = "
                "wrap_serialize_field_with(params, field.ty, path, &field_expr);` with "
                "`field_expr = wrap_newtype_field(params, field.ty, path, &field_expr);`.\n"
                "2. Add this function at the end of the file:\n\n"
                f"```rust\n{WRAP_NEWTYPE_FIELD}```"
            ),
            steps=(
                Replace(
                    file=SER,
                    after="fn serialize_newtype_struct(",
                    old=(
                        "field_expr = wrap_serialize_field_with("
                        "params, field.ty, path, &field_expr);"
                    ),
                    new="field_expr = wrap_newtype_field(params, field.ty, path, &field_expr);",
                ),
                Append(file=SER, text=f"\n{WRAP_NEWTYPE_FIELD}"),
            ),
            present=(
                Marker(SER, "fn wrap_newtype_field("),
                Marker(
                    SER,
                    "field_expr = wrap_newtype_field(params, field.ty, path, &field_expr);",
                ),
            ),
            absent=(),
            added=(sym(SER, "wrap_newtype_field"),),
            removed=(sym(SER, "serialize_newtype_struct"),),
        ),
        notes=(
            "Before the change 7 functions call it, one call each (rg); the change routes "
            "serialize_newtype_struct through wrap_newtype_field."
        ),
    ),
    Task(
        id="serde-edit-with-bound",
        repo="serde",
        category="edit",
        answer_kind="symbols",
        question=(
            "After the change, which functions in `serde_derive/src/` call `bound::with_bound` "
            "directly?"
        ),
        expected=(sym(DE, "build_generics"), sym(SER, "with_serialize_bound")),
        edit=Edit(
            instructions=(
                "In `serde_derive/src/ser.rs`:\n\n"
                "1. In `build_generics`, replace `None => bound::with_bound(` with "
                "`None => with_serialize_bound(` (the arguments stay as they are).\n"
                "2. Add this function at the end of the file:\n\n"
                f"```rust\n{WITH_SERIALIZE_BOUND}```"
            ),
            steps=(
                Replace(
                    file=SER,
                    after="fn build_generics(",
                    old="None => bound::with_bound(",
                    new="None => with_serialize_bound(",
                ),
                Append(file=SER, text=f"\n{WITH_SERIALIZE_BOUND}"),
            ),
            present=(
                Marker(SER, "fn with_serialize_bound("),
                Marker(SER, "None => with_serialize_bound("),
            ),
            absent=(Marker(SER, "None => bound::with_bound("),),
            added=(sym(SER, "with_serialize_bound"),),
            removed=(sym(SER, "build_generics"),),
        ),
        notes=(
            "Before the change build_generics in de.rs (two calls) and build_generics in ser.rs "
            "call bound::with_bound; both files define a function named build_generics."
        ),
    ),
    Task(
        id="serde-edit-deserialize-seq",
        repo="serde",
        category="edit",
        answer_kind="symbols",
        question=(
            "After the change, which functions in `serde_derive/src/` call `deserialize_seq` "
            "directly?"
        ),
        expected=symbols(DE, ("deserialize_struct", "deserialize_tuple_seq")),
        edit=Edit(
            instructions=(
                "In `serde_derive/src/de.rs`:\n\n"
                "1. In `deserialize_tuple`, replace `let visit_seq = Stmts(deserialize_seq(` with "
                "`let visit_seq = Stmts(deserialize_tuple_seq(` (the arguments stay as they "
                "are).\n"
                "2. Add this function at the end of the file:\n\n"
                f"```rust\n{DESERIALIZE_TUPLE_SEQ}```"
            ),
            steps=(
                Replace(
                    file=DE,
                    after="fn deserialize_tuple(",
                    old="let visit_seq = Stmts(deserialize_seq(",
                    new="let visit_seq = Stmts(deserialize_tuple_seq(",
                ),
                Append(file=DE, text=f"\n{DESERIALIZE_TUPLE_SEQ}"),
            ),
            present=(
                Marker(DE, "fn deserialize_tuple_seq("),
                Marker(DE, "let visit_seq = Stmts(deserialize_tuple_seq("),
            ),
            absent=(),
            added=(sym(DE, "deserialize_tuple_seq"),),
            removed=(sym(DE, "deserialize_tuple"),),
        ),
        notes=(
            "Before the change deserialize_tuple and deserialize_struct call deserialize_seq "
            "(rg); deserialize_seq_in_place is a different function."
        ),
    ),
)

# --- Django 5.1 (product pilots only; not part of the preregistered 24) ------------------------

HTML = "django/utils/html.py"
DEFAULTFILTERS = "django/template/defaultfilters.py"

DJANGO_TASKS: tuple[Task, ...] = (
    Task(
        id="django-trap-html-escape",
        repo="django",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions or methods in `django/` call Django's HTML `escape` function "
            "(`django.utils.html.escape`) directly? Calls to other functions named `escape` "
            "(for example `re.escape`, `glob.escape` or the standard library's `html.escape`) "
            "do not count."
        ),
        expected=(
            sym("django/contrib/auth/admin.py", "user_change_password"),
            sym("django/contrib/syndication/views.py", "item_title"),
            sym("django/forms/utils.py", "get_json_data"),
            sym("django/template/base.py", "get_exception_info"),
            *symbols(DEFAULTFILTERS, ("linenumbers", "force_escape", "linebreaksbr")),
            sym("django/template/defaulttags.py", "render"),
            *symbols(HTML, ("conditional_escape", "linebreaks", "handle_word")),
        ),
        edit=None,
        notes=(
            "Python ast over django/**/*.py: names bound by `from django.utils.html import "
            "escape` (or the local definition in django/utils/html.py) called directly; 21 call "
            "sites in 11 functions. `rg '\\bescape\\(' django` gives 43 lines: the rest are "
            "re.escape, glob.escape, stdlib html.escape (django/utils/text.py imports `escape` "
            "from the standard library `html`, django/test/html.py calls html.escape), a "
            "docstring, the definition and MySQL's connection.escape."
        ),
    ),
)

DJANGO_HELD_OUT_TASKS: tuple[Task, ...] = (
    Task(
        id="django-callers-make-aware",
        repo="django",
        category="callers",
        answer_kind="symbols",
        question=(
            "Which functions or methods in `django/` call `django.utils.timezone.make_aware` "
            "directly (either as `make_aware(...)` or as `timezone.make_aware(...)`)?"
        ),
        expected=(
            sym("django/contrib/admin/views/main.py", "get_filters"),
            sym("django/contrib/sitemaps/views.py", "_get_latest_lastmod"),
            sym("django/contrib/syndication/views.py", "get_feed"),
            sym("django/db/backends/base/operations.py", "year_lookup_bounds_for_datetime_field"),
            sym("django/db/backends/mysql/operations.py", "convert_datetimefield_value"),
            sym("django/db/backends/oracle/operations.py", "convert_datetimefield_value"),
            sym("django/db/backends/sqlite3/operations.py", "convert_datetimefield_value"),
            *symbols("django/db/models/fields/__init__.py", ("to_python", "get_prep_value")),
            sym("django/db/models/functions/datetime.py", "convert_value"),
            sym("django/forms/utils.py", "from_current_timezone"),
            sym("django/http/response.py", "set_cookie"),
            sym("django/templatetags/tz.py", "do_timezone"),
            sym("django/utils/dateformat.py", "r"),
            sym_with("django/views/decorators/http.py", "condition", ("_pre_process_request",)),
            sym("django/views/generic/dates.py", "_make_date_lookup_arg"),
        ),
        edit=None,
        notes=(
            "Python ast over django/**/*.py: names bound by `from django.utils.timezone import "
            "make_aware` and `timezone.make_aware` through `from django.utils import timezone`; "
            "19 call sites in 16 functions (a call inside a nested function maps to the outermost "
            "named function). Held out for the 2026-11-10 iteration-2 pilot: chosen before any "
            "CCM packet for it was looked at."
        ),
    ),
)

DJANGO_ITERATION3_TASKS: tuple[Task, ...] = (
    Task(
        id="django-callers-get-random-string",
        repo="django",
        category="callers",
        answer_kind="symbols",
        question=(
            "Which functions or methods in `django/` call `django.utils.crypto.get_random_string` "
            "directly?"
        ),
        expected=(
            *symbols(
                "django/contrib/auth/hashers.py", ("verify_password", "make_password", "salt")
            ),
            *symbols(
                "django/contrib/sessions/backends/base.py",
                ("_get_new_session_key", "_aget_new_session_key"),
            ),
            sym("django/core/files/storage/base.py", "get_alternative_name"),
            sym("django/core/management/utils.py", "get_random_secret_key"),
            sym("django/db/backends/oracle/creation.py", "_test_database_passwd"),
            sym("django/middleware/csrf.py", "_get_new_csrf_string"),
        ),
        edit=None,
        notes=(
            "Python ast over django/**/*.py: names bound by `from django.utils.crypto import "
            "get_random_string` (and module-attribute calls); 9 call sites in 9 functions. Held "
            "out for the 2026-11-10 iteration-3 pilot: chosen before any CCM packet for it was "
            "looked at."
        ),
    ),
)

DJANGO_ITERATION4_TASKS: tuple[Task, ...] = (
    Task(
        id="django-trap-capfirst",
        repo="django",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions or methods in `django/` call `django.utils.text.capfirst` directly? "
            "Calls to the template filter `django.template.defaultfilters.capfirst` do not count."
        ),
        expected=(
            *symbols(
                "django/contrib/admin/options.py", ("_get_action_description", "history_view")
            ),
            sym("django/contrib/admin/sites.py", "_build_app_dict"),
            sym("django/contrib/admin/templatetags/admin_list.py", "date_hierarchy"),
            sym_with("django/contrib/admin/utils.py", "get_deleted_objects", ("format_callback",)),
            sym("django/contrib/auth/forms.py", "__init__"),
            *symbols(
                "django/contrib/auth/management/commands/createsuperuser.py",
                ("handle", "_get_input_message", "_validate_username"),
            ),
            *symbols("django/db/models/base.py", ("date_error_message", "unique_error_message")),
            sym("django/db/models/fields/__init__.py", "formfield"),
            sym("django/forms/models.py", "add_fields"),
            sym("django/test/selenium.py", "__new__"),
        ),
        edit=None,
        notes=(
            "Python ast over django/**/*.py: names bound by `from django.utils.text import "
            "capfirst` (and module-attribute calls); 22 call sites in 14 functions. "
            "django/contrib/admin/helpers.py imports the template filter capfirst from "
            "django.template.defaultfilters (two calls), which does not count. Held out for the "
            "2026-11-10 iteration-4 pilot: chosen before any CCM packet for it was looked at."
        ),
    ),
)

DJANGO_ITERATION5_TASKS: tuple[Task, ...] = (
    Task(
        id="django-callers-iri-to-uri",
        repo="django",
        category="callers",
        answer_kind="symbols",
        question=(
            "Which functions or methods in `django/` call `django.utils.encoding.iri_to_uri` "
            "directly?"
        ),
        expected=(
            sym("django/contrib/flatpages/models.py", "get_absolute_url"),
            sym("django/contrib/syndication/views.py", "add_domain"),
            *symbols("django/http/request.py", ("_get_full_path", "build_absolute_uri")),
            sym("django/http/response.py", "__init__"),
            sym("django/template/defaultfilters.py", "iriencode"),
            sym("django/templatetags/static.py", "handle_simple"),
            *symbols("django/utils/feedgenerator.py", ("__init__", "add_item")),
        ),
        edit=None,
        notes=(
            "Python ast over django/**/*.py: names bound by `from django.utils.encoding import "
            "iri_to_uri` (and module-attribute calls); 13 call sites in 10 functions "
            "(feedgenerator has two __init__ methods, SyndicationFeed and Enclosure, scored as "
            "one file-level name). A mention in a docstring in django/utils/http.py is not a "
            "call. Held out for the 2026-11-10 iteration-5 pilot: chosen before any CCM packet "
            "for it was looked at."
        ),
    ),
)

DJANGO_ITERATION6_TASKS: tuple[Task, ...] = (
    Task(
        id="django-callers-constant-time-compare",
        repo="django",
        category="callers",
        answer_kind="symbols",
        question=(
            "Which functions or methods in `django/` call "
            "`django.utils.crypto.constant_time_compare` directly?"
        ),
        expected=(
            *symbols("django/contrib/auth/__init__.py", ("login", "get_user")),
            sym("django/contrib/auth/hashers.py", "verify"),
            sym("django/contrib/auth/tokens.py", "check_token"),
            sym("django/core/signing.py", "unsign"),
            sym("django/middleware/csrf.py", "_does_token_match"),
        ),
        edit=None,
        notes=(
            "Python ast over django/**/*.py: names bound by `from django.utils.crypto import "
            "constant_time_compare`; 10 call sites in 9 functions (four password hashers each "
            "have a `verify` method, scored as one file-level name). Held out for the "
            "2026-11-10 large-repository pilot (preregistration amendment 9)."
        ),
    ),
    Task(
        id="django-trap-urlencode",
        repo="django",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions or methods in `django/` call `django.utils.http.urlencode` directly? "
            "Calls to the standard-library `urllib.parse.urlencode`, to the `QueryDict.urlencode` "
            "method and to the `urlencode` template filter do not count."
        ),
        expected=(
            sym("django/contrib/admin/options.py", "get_preserved_filters"),
            sym("django/contrib/admin/templatetags/admin_urls.py", "add_preserved_filters"),
            sym("django/contrib/admin/views/main.py", "get_query_string"),
            sym("django/contrib/admin/widgets.py", "get_context"),
            *symbols("django/test/client.py", ("__init__", "generic")),
        ),
        edit=None,
        notes=(
            "Python ast over django/**/*.py: names bound by `from django.utils.http import "
            "urlencode`; 7 calls in 7 functions. django/http/request.py, django/utils/html.py "
            "and django/test/testcases.py call the standard-library urlencode; "
            "`request.GET.urlencode()` and `query_dict.urlencode()` are QueryDict methods; "
            "django/template/defaultfilters.py defines the filter. Held out for the 2026-11-10 "
            "large-repository pilot (preregistration amendment 9)."
        ),
    ),
)

TOKIO_TASKS: tuple[Task, ...] = (
    Task(
        id="tokio-callers-sleep-until",
        repo="tokio",
        category="callers",
        answer_kind="symbols",
        question=(
            "Which functions or methods in the library crates of this workspace (`tokio`, "
            "`tokio-util`, `tokio-stream`, `tokio-test`; not tests, benches or examples) call "
            "`tokio::time::sleep_until` directly?"
        ),
        expected=(
            sym("tokio-test/src/stream_mock.rs", "poll_next"),
            *symbols("tokio-test/src/io.rs", ("poll_read", "poll_write")),
            sym("tokio-stream/src/stream_ext/timeout.rs", "new"),
            sym("tokio-stream/src/stream_ext/throttle.rs", "throttle"),
            *symbols("tokio-util/src/time/delay_queue.rs", ("insert_at", "remove", "poll_idx")),
            sym("tokio/src/time/interval.rs", "internal_interval_at"),
            sym("tokio/src/time/timeout.rs", "timeout_at"),
        ),
        edit=None,
        notes=(
            "rg '(^|[^\\w.])sleep_until\\s*\\(' over the library crates, each call read in "
            "its enclosing function: 11 call sites in 10 functions (two cfg branches in "
            "internal_interval_at); mentions in doc comments are not calls. Held out "
            "(preregistration amendment 10)."
        ),
    ),
    Task(
        id="tokio-trap-catch-unwind",
        repo="tokio",
        category="trap",
        answer_kind="symbols",
        question=(
            "`tokio/src/sync/task/atomic_waker.rs` defines a helper function `catch_unwind`. Which "
            "functions or methods in the library crates of this workspace call that helper "
            "directly? Calls to `std::panic::catch_unwind` do not count."
        ),
        expected=(sym("tokio/src/sync/task/atomic_waker.rs", "do_register"),),
        edit=None,
        notes=(
            "The helper is nested inside AtomicWaker::do_register and is called three times "
            "there (lines 195, 224, 252); the other eleven `catch_unwind(` calls in the library "
            "crates (signal/reusable_box.rs, sync/watch.rs, runtime/task/harness.rs, "
            "tokio-test/src/io.rs and the helper's own body) call std::panic::catch_unwind. Held "
            "out (preregistration amendment 10)."
        ),
    ),
)

TOKIO_HELD_OUT_TASKS: tuple[Task, ...] = (
    Task(
        id="tokio-callers-async-op",
        repo="tokio",
        category="callers",
        answer_kind="symbols",
        question=(
            "Which functions or methods in the library crates of this workspace call the "
            "`async_op` function defined in `tokio/src/util/trace.rs` directly?"
        ),
        expected=(
            sym("tokio/src/sync/barrier.rs", "wait"),
            *symbols("tokio/src/sync/mutex.rs", ("lock", "lock_owned")),
            *symbols(
                "tokio/src/sync/semaphore.rs",
                ("acquire", "acquire_many", "acquire_owned", "acquire_many_owned"),
            ),
            *symbols("tokio/src/sync/rwlock.rs", ("read", "read_owned", "write", "write_owned")),
            sym("tokio/src/sync/oneshot.rs", "closed"),
            sym("tokio/src/time/interval.rs", "tick"),
        ),
        edit=None,
        notes=(
            "rg '(^|[^\\w.])async_op\\s*\\(' over the library crates (all calls are "
            "`trace::async_op(`), each read in its enclosing method: 13 calls in 13 methods. "
            "Held out (preregistration amendment 11)."
        ),
    ),
    Task(
        id="tokio-trap-yield-now",
        repo="tokio",
        category="trap",
        answer_kind="symbols",
        question=(
            "Which functions or methods in the library crates of this workspace call "
            "`tokio::task::yield_now` (defined in `tokio/src/task/yield_now.rs`) directly? Calls "
            "to `std::thread::yield_now`, to the `loom` wrapper `crate::loom::thread::yield_now` "
            "and to scheduler `yield_now` methods do not count."
        ),
        expected=(
            sym("tokio/src/signal/registry.rs", "smoke"),
            sym("tokio/src/time/clock.rs", "advance"),
            sym("tokio-util/src/task/spawn_pinned.rs", "run"),
            *symbols(
                "tokio/src/runtime/scheduler/multi_thread/trace.rs",
                ("start_trace_request", "end_trace_request"),
            ),
            *symbols(
                "tokio/src/runtime/scheduler/multi_thread_alt/trace.rs",
                ("start_trace_request", "end_trace_request"),
            ),
        ),
        edit=None,
        notes=(
            "Seven calls in five files: `crate::task::yield_now()` in signal/registry.rs, "
            "time/clock.rs and both scheduler trace.rs files, `tokio::task::yield_now()` in "
            "tokio-util spawn_pinned.rs. Not counted: std::thread::yield_now in "
            "loom/std/barrier.rs, `thread::yield_now()` in sync/mpsc/list.rs and "
            "`crate::loom::thread::yield_now()` in sync/mpsc/block.rs (both the loom wrapper, "
            "which calls std::hint::spin_loop). Held out (preregistration amendment 11)."
        ),
    ),
)

TASKS: tuple[Task, ...] = FLASK_TASKS + EXPRESS_TASKS + SERDE_TASKS
# Ürün pilotlarının seçebildiği tüm görevler; `--tasks all` yalnız ön kayıtlı 24 görevdir.
KNOWN_TASKS: tuple[Task, ...] = (
    TASKS
    + DJANGO_TASKS
    + DJANGO_HELD_OUT_TASKS
    + DJANGO_ITERATION3_TASKS
    + DJANGO_ITERATION4_TASKS
    + DJANGO_ITERATION5_TASKS
    + DJANGO_ITERATION6_TASKS
    + TOKIO_TASKS
    + TOKIO_HELD_OUT_TASKS
)
