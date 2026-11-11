# Selection of the amendment-20 held-out set

Written before any run of these tasks. No CCM packet, `context` answer or hook answer for any
of the thirteen targets was produced or read. The hook replay in this session used the
amendment-17 and amendment-18 transcripts only.

```bash
EX_PY=escape,make_aware,get_random_string,capfirst,iri_to_uri,constant_time_compare,urlencode,is_aware,make_hashable,get_connection,get_resolver,check_cv,gen_batches,shuffle,get_config,log_response,get_default_timezone,get_text_list,get_docs_version,resolve,indexable,empirical_covariance,gen_even_slices,check_scoring,normalize,http_date,get_script_prefix,get_template,is_regressor,get_current_timezone,prefix_validation_error,make_model_tuple,get_default_renderer,resolve_url,conditional_escape,load_backend,compute_sample_weight,softmax,label_binarize,device,supported_float_dtypes,check_non_negative,no_style,flatten_fieldsets,call_command,get_user_model,safe_join,get_current_site,select_template,compute_class_weight,as_float_array,get_scorer_names,svd_flip,pairwise_distances,accuracy_score
PYTHONPATH=. uv run python scripts/select_heldout.py corpus/django django 20261114 7 1 $EX_PY
PYTHONPATH=. uv run python scripts/select_heldout.py corpus/scikit-learn sklearn 20261114 7 1 $EX_PY
```

Callers picks with a module-level call (`<module>` in the gold) are skipped in draw order, and
the first five that remain are used.

| Repository | Callers drawn (in order) | Callers used | Impact | Trap |
|---|---|---|---|---|
| Django | `deconstructible`, `double_output`, `make_naive`, `number_format`, `get_version_tuple`, `strip_quotes`, `validate_file_name` | the last five | `is_naive` | `django.db.migrations.utils.resolve_relation` |
| scikit-learn | `check_X_y`, `check_scalar`, `pairwise_distances_chunked`, `pairwise_kernels`, `has_fit_parameter`, `stable_cumsum`, `check_matplotlib_support` | the first five | `randomized_svd` | none (pool empty) |

## Intermediate draw

With five callers per repository, the same seed drew Django `deconstructible`,
`double_output`, `make_naive`, `number_format` and `get_version_tuple`; impact `is_naive`; trap
`resolve_relation`. For scikit-learn it drew the five callers above, impact `load_descr` and no
trap. A hand check of the gold found module-level calls for `deconstructible` (`@deconstructible(...)`
on classes) and `double_output` (module-level assignments in `gdal/prototypes`). The skip
rule was set then, and the draw was repeated with seven callers. Drawing seven advanced the
generator, so scikit-learn's impact pick became `randomized_svd`.
