---
description: Strict Code Audit workflow. Evaluates code against OWASP security standards, SOLID principles, and performance metrics. Acts as a 'Gatekeeper' before production, flagging critical issues, logical bugs, and technical debt with a grading system.
---

# WORKFLOW: CODE REVIEW & AUDIT

Triggered by "review", "audit", "check".

## 0. AUDIT RULES
- **Reference:** Compare code strictly against definitions in `.gemini/GEMINI.md`.
- **Mindset:** Brutally honest. No "looks good" if it violates SOLID.

## 1. SCORING CRITERIA
- 🔴 **BLOCKER:** Security risk (OWASP), Secrets exposed, Logic error.
- 🟠 **WARNING:** Performance (N+1), Messy code, Missing types.
- 🔵 **NITPICK:** Naming, formatting.

## 2. REPORTING (TURKISH)
1.  **Puan:** X/100.
2.  **Kritik Hatalar:** List blockers with line numbers.
3.  **Düzeltme Önerisi:** Rewrite the worst part correctly.