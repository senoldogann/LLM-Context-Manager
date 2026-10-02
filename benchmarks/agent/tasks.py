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

TASKS: tuple[Task, ...] = FLASK_TASKS + EXPRESS_TASKS + SERDE_TASKS
