# Incomplete run set

The harness process was stopped by the supervising session's 2-hour limit for background
commands while the Django runs were rebuilding their index with local embeddings (the frozen
`b6d055d` harness indexes from scratch in every B/C/F run, about 1 hour per Django run on this
host under load). The Django task of this set was therefore not completed in the CCM arms;
only the runs listed under `runs/` exist, and the report covers only those. The same Django task
was measured again in iteration 2 (`../iter2-20261110/`) with an index built once and copied.
