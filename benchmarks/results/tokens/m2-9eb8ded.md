# Token cost of answers

- v1: harness `4bc3145`, 2026-10-02T03:26:36+00:00
- v2: harness `9eb8ded`, 2026-10-02T04:45:27+00:00

Estimated tokens = bytes / 4.

## Fixed overhead per session

| Version | tools/list bytes | SKILL.md bytes |
|---|---|---|
| v1 | 9333 | 25993 |
| v2 | 6596 | 3990 |

## Per question kind

| Repo | Kind | Version | Questions | Calls | Bytes | Est. tokens | Median bytes |
|---|---|---|---|---|---|---|---|
| django | callers | v1 | 5 | 24 | 97855 | 24463 | 15613 |
| django | callers | v2 | 5 | 21 | 18259 | 4564 | 1889 |
| django | explain | v1 | 5 | 43 | 154130 | 38532 | 21712 |
| django | explain | v2 | 5 | 21 | 40502 | 10125 | 6060 |
| django | impact | v1 | 2 | 2 | 17238 | 4309 | 8619 |
| django | impact | v2 | 2 | 2 | 7449 | 1862 | 3724 |
| django | map | v2 | 1 | 1 | 4007 | 1001 | 4007 |
| flask | callers | v1 | 5 | 24 | 101998 | 25499 | 17793 |
| flask | callers | v2 | 5 | 23 | 18421 | 4605 | 2021 |
| flask | explain | v1 | 5 | 43 | 139654 | 34913 | 24851 |
| flask | explain | v2 | 5 | 23 | 48229 | 12057 | 6668 |
| flask | impact | v1 | 2 | 2 | 16037 | 4009 | 8018 |
| flask | impact | v2 | 2 | 2 | 6914 | 1728 | 3457 |
| flask | map | v2 | 1 | 1 | 4075 | 1018 | 4075 |

## Parity of callers (v1 locations found in v2 answers)

| Repo | Found | v1 total | Share |
|---|---|---|---|
| django | 78 | 79 | 99% |
| flask | 85 | 85 | 100% |
