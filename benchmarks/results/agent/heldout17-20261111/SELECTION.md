# Selection of the amendment-17 held-out set

Written before any run of these tasks. No CCM packet, `context` answer or hook answer for any of
the thirteen targets was produced or read during selection. The only CCM output in this
session was about earlier tasks: the hook replay of the amendment-15 transcripts, and the
`context` packets for `try_with` and `unbounded_channel`.

## Final commands

Run in `benchmarks/` against the pinned corpus (Django 5.1 `373cb30`, scikit-learn 1.5.2
`156ef14`, Tokio 1.41.1 `bb7ca75`):

```bash
EX_PY=escape,make_aware,get_random_string,capfirst,iri_to_uri,constant_time_compare,urlencode,is_aware,make_hashable,get_connection,get_resolver,check_cv,gen_batches,shuffle,get_config
PYTHONPATH=. uv run python scripts/select_heldout.py corpus/django django 20261111 3 1 $EX_PY
PYTHONPATH=. uv run python scripts/select_heldout.py corpus/scikit-learn sklearn 20261111 3 1 $EX_PY
EX_RS=sleep_until,catch_unwind,async_op,yield_now,with_scheduler,poll_proceed,new_unnamed,unbounded_channel,try_with,enter_runtime
uv run python scripts/rust_callers_gold.py corpus/tokio tokio,tokio-util,tokio-stream,tokio-test,tokio-macros --select 20261111 $EX_RS
```

The excluded names are the targets of every earlier task in these repositories. Output:

| Repository | Callers pool | Callers picked | Impact pool | Impact picked | Trap pool | Trap picked |
|---|---|---|---|---|---|---|
| Django | 27 | `log_response`, `get_default_timezone`, `get_text_list` | 9 | `get_docs_version` | 6 | `django.urls.base.resolve` |
| scikit-learn | 26 | `indexable`, `empirical_covariance`, `gen_even_slices` | 11 | `check_scoring` | 1 | `sklearn.preprocessing._data.normalize` |
| Tokio | 2 | `ReadBuf::uninit`, `signal::registry::globals` | 0 | none | 3 | `Registration::new_with_interest_and_handle` |

Gold answers come from `scripts/python_callers_gold.py <root> <package> <module> <name>` and
`scripts/rust_callers_gold.py <root> <crates> <path>:<name>`. Every Tokio call site was read by
hand. An independent review checked ten targets against the source.

## How the rules changed, with every intermediate draw

Each change was made before any run of the new tasks. Each was caused by an empty pool or by a
wrong Rust gold found in a hand check. None was made to obtain a particular target.

1. **Python, two callers tasks, trap rule (a) only** (a second top-level definition of the same
   name):
   - Django: `log_response`, `get_default_timezone`; impact `http_date`; trap
     `get_script_prefix`.
   - scikit-learn: `indexable`, `empirical_covariance`; impact `is_regressor`; trap pool empty.
2. **Trap rule (b) added** (the name is also a method). Django's trap became `get_template`;
   the scikit-learn trap pool was still empty, because the 4–60 text-count prefilter removed
   method names.
3. **Trap rule (c) added** (the same name on an externally imported module, e.g.
   `unicodedata.normalize`), and the prefilter dropped for rules (b) and (c). scikit-learn's
   trap became `normalize`.
4. **Tokio, first scanner** (no name resolution): callers `globals`, `async_trace_leaf`; impact
   and trap `rc_cell.rs:replace`. The `replace` counts included `std::mem::replace`, so name
   resolution was added: a qualifier must be the definition's module or impl type, and a bare
   call needs a `use` of the name.
5. **Tokio with resolution:** callers `uninit`, `globals`; impact `new_current_thread`; trap
   pool empty. Trap rule (a) was added for Rust (two definitions without `self` in different
   files). Trap: `poll_evented.rs:new_with_interest`.
6. **Hand check of the Tokio gold:**
   - `new_current_thread` listed functions inside `#[cfg(all(test, …))]` modules, so the
     scanner now recognises any `cfg` that mentions `test` positively.
   - `new_with_interest` counted `Self::new_with_interest` inside `impl Aio`, so `Self::` now
     counts only inside the definition's own impl.
   - After both fixes, Tokio's impact pool was empty (`new_current_thread` fell to 5 call
     sites) and the trap became `registration.rs:new_with_interest_and_handle`.
   - The scanner still reproduces the amendment-15 gold of `new_unnamed`, `unbounded_channel`
     and `async_op`.
7. **Three callers tasks per Python repository**, so the set keeps at least twelve tasks
   without a Tokio impact task. Sampling three callers advances the random generator, which
   changed the Python impact and trap picks to `get_docs_version`/`resolve` and
   `check_scoring`/`normalize`. This is the final set.

Before the selection script existed, a quick candidate scan in the session scratchpad listed the
Python candidates with 6–20 call sites (names and counts only). It was not used to pick tasks.
