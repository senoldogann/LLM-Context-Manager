# Selection of the amendment-18 held-out set

Written before any run of these tasks. No CCM packet, `context` answer or hook answer for any
of the thirteen targets was produced or read. The only hook replay in this session used the
amendment-17 transcripts and their targets.

## Final commands

Run in `benchmarks/` against the pinned corpus (Django 5.1 `373cb30`, scikit-learn 1.5.2
`156ef14`, Tokio 1.41.1 `bb7ca75`):

```bash
EX_PY=escape,make_aware,get_random_string,capfirst,iri_to_uri,constant_time_compare,urlencode,is_aware,make_hashable,get_connection,get_resolver,check_cv,gen_batches,shuffle,get_config,log_response,get_default_timezone,get_text_list,get_docs_version,resolve,indexable,empirical_covariance,gen_even_slices,check_scoring,normalize,http_date,get_script_prefix,get_template,is_regressor
PYTHONPATH=. uv run python scripts/select_heldout.py corpus/django django 20261112 5 1 $EX_PY
PYTHONPATH=. uv run python scripts/select_heldout.py corpus/scikit-learn sklearn 20261112 5 1 $EX_PY
EX_RS=sleep_until,catch_unwind,async_op,yield_now,with_scheduler,poll_proceed,new_unnamed,unbounded_channel,try_with,enter_runtime,uninit,globals,new_with_interest_and_handle,async_trace_leaf,replace,new_current_thread,new_with_interest
uv run python scripts/rust_callers_gold.py corpus/tokio tokio,tokio-util,tokio-stream,tokio-test,tokio-macros --select 20261112 $EX_RS
```

The excluded names are every earlier task target, plus the names seen in amendment 17's
intermediate draws: `http_date`, `get_script_prefix`, `get_template` and `is_regressor` for
Python, and `async_trace_leaf`, `replace`, `new_current_thread` and `new_with_interest` for
Rust.

| Repository | Callers pool | Callers picked | Impact pool | Impact picked | Trap pool | Trap picked |
|---|---|---|---|---|---|---|
| Django | 23 | `get_current_timezone`, `prefix_validation_error`, `make_model_tuple`, `get_default_renderer`, `resolve_url` | 6 | `conditional_escape` | 3 | `django.contrib.auth.load_backend` |
| scikit-learn | 22 | `compute_sample_weight`, `softmax`, `label_binarize`, `device`, `supported_float_dtypes` | 9 | `check_non_negative` | 0 | none |
| Tokio | 0 | none | 0 | none | 0 | none |

## Intermediate draws

1. **Four callers tasks per Python repository**, same seed:
   - Django: `get_current_timezone`, `prefix_validation_error`, `make_model_tuple`,
     `get_default_renderer`; impact `no_style`; trap `load_backend`.
   - scikit-learn: `compute_sample_weight`, `softmax`, `label_binarize`, `device`; impact
     `check_non_negative`; no trap.
   - Tokio: one trap, `coop.rs:unconstrained` (`Budget::unconstrained`).
2. **Hand check of the Tokio trap.** The scanner had accepted the bare `unconstrained(jh)` in
   `task/join_set.rs`. That call goes to `task::unconstrained`, which the file imports from
   `crate::task`. A bare call now needs a `use` statement that names a module on the
   definition's path. With that fix, all three Tokio pools are empty, and the amendment-17
   Tokio gold (`uninit`, `globals`, `new_with_interest_and_handle`) and the three
   amendment-15 checks are unchanged.
3. **Five callers tasks per Python repository**, set before the draw so that the set keeps at
   least twelve tasks without Tokio. Sampling five callers advances the generator, so
   Django's impact pick became `conditional_escape`. This is the final set.
