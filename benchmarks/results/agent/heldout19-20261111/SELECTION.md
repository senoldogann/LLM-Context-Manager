# Selection of the amendment-19 held-out set

Written before any run of these tasks. No CCM packet, `context` answer or hook answer for any
of the twelve targets was produced or read. The `context` replay in this session used the
amendment-17 and amendment-18 queries only.

```bash
EX_PY=escape,make_aware,get_random_string,capfirst,iri_to_uri,constant_time_compare,urlencode,is_aware,make_hashable,get_connection,get_resolver,check_cv,gen_batches,shuffle,get_config,log_response,get_default_timezone,get_text_list,get_docs_version,resolve,indexable,empirical_covariance,gen_even_slices,check_scoring,normalize,http_date,get_script_prefix,get_template,is_regressor,get_current_timezone,prefix_validation_error,make_model_tuple,get_default_renderer,resolve_url,conditional_escape,load_backend,compute_sample_weight,softmax,label_binarize,device,supported_float_dtypes,check_non_negative,no_style
PYTHONPATH=. uv run python scripts/select_heldout.py corpus/django django 20261113 2 3 $EX_PY
PYTHONPATH=. uv run python scripts/select_heldout.py corpus/scikit-learn sklearn 20261113 2 4 $EX_PY
```

The excluded names are every earlier task target plus every name seen in an earlier
intermediate draw.

| Repository | Callers pool | Callers picked | Impact pool | Impact picked | Trap pool | Trap picked |
|---|---|---|---|---|---|---|
| Django | 18 | `flatten_fieldsets`, `call_command` | 4 | `get_user_model`, `safe_join`, `get_current_site` | 2 | `django.template.loader.select_template` |
| scikit-learn | 17 | `compute_class_weight`, `as_float_array` | 8 | `get_scorer_names`, `svd_flip`, `pairwise_distances`, `accuracy_score` | 0 | none |

There were no intermediate draws. Before the draw, a run with seed 0 printed only the pool sizes
(Django 18/4/2, scikit-learn 17/8/0) to fix the task counts; it printed no picks. Tokio was not
drawn: its pools are empty under the task rules (amendment 18). Gold answers come from
`scripts/python_callers_gold.py`, and none of the callers tasks has a call inside a nested
function.
