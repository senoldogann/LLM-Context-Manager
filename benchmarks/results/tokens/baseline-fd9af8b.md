# Token cost of answers

- v1: harness `4bc3145`, 2026-10-02T03:26:36+00:00

Estimated tokens = bytes / 4.

## Fixed overhead per session

| Version | tools/list bytes | SKILL.md bytes |
|---|---|---|
| v1 | 9333 | 25993 |

## Per question kind

| Repo | Kind | Version | Questions | Calls | Bytes | Est. tokens | Median bytes |
|---|---|---|---|---|---|---|---|
| django | callers | v1 | 5 | 24 | 97855 | 24463 | 15613 |
| django | explain | v1 | 5 | 43 | 154130 | 38532 | 21712 |
| django | impact | v1 | 2 | 2 | 17238 | 4309 | 8619 |
| flask | callers | v1 | 5 | 24 | 101998 | 25499 | 17793 |
| flask | explain | v1 | 5 | 43 | 139654 | 34913 | 24851 |
| flask | impact | v1 | 2 | 2 | 16037 | 4009 | 8018 |
