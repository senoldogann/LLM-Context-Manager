---
description: The ultimate 'Pre-Flight Check' before production. Orchestrates Code Audit, Refactoring, and Testing workflows sequentially on critical modules. Acts as a Release Manager to ensure 100% system integrity.
---

# WORKFLOW: PRE-PRODUCTION RELEASE PROTOCOL (THE GAUNTLET)

Triggered by "full system check", "prepare for release", "run all checks".

## 0. STRATEGY: DIVIDE & CONQUER
**WARNING:** Context limits are real. Do not scan the whole repo.
**Action:** Ask user to specify the **Critical Path** or **Target Module** (e.g., "Check `Auth` module").

## PHASE 1: THE AUDIT (Static Analysis)
1.  **Security:** Scan for hardcoded secrets, SQLi risks, and exposed endpoints.
2.  **Architecture:** Check for violations against `.gemini/GEMINI.md`.
3.  **Outcome:** If **BLOCKER** found -> STOP -> Call `deep-debugging`.

## PHASE 2: SANITIZATION (Refactoring)
1.  **Complexity:** Flag functions with Cyclomatic Complexity > 10.
2.  **Types:** Eliminate `any`. Enforce strict DTOs.
3.  **Clean Code:** Apply DDD naming conventions.

## PHASE 3: ASSURANCE (Testing)
1.  **Gaps:** Find logic branches with 0% coverage.
2.  **Edge Cases:** Add tests for `null`, `undefined`, and empty arrays.
3.  **Mocks:** Verify no external calls are leaking in unit tests.

## PHASE 4: FINAL VERIFICATION
1.  **Build:** Verify build command (npm/go/cargo) passes without warnings.
2.  **Config:** Check if all necessary Environment Variables are documented in `.env.example`.

## 5. MISSION REPORT (TURKISH)
1.  **Modül Durumu:** [PASS/FAIL]
2.  **Risk Analizi:** Is this ready for production?
3.  **Karar:** **RELEASE CANDIDATE APPROVED** or **REJECTED**.