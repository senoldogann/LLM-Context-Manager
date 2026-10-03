# Single S check run: wrapper not invoked

Commit `c0ce0a2`. The wrapper was first in the environment PATH and HOME/.zprofile, but with the
Codex shell snapshot on it was still not invoked (the record's failure says so). A one-command probe
with `--disable shell_snapshot` then invoked it; commit `3814a87` disables the snapshot in both arms.
