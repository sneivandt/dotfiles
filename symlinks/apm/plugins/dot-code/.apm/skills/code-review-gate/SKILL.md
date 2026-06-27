---
name: code-review-gate
description: Use when reviewing code changes. Produce high-signal, evidence-backed findings only: real bugs, security issues, maintainability risks, test gaps, cross-platform breakage, or bad-practice patterns that materially affect correctness.
---

# Code Review Gate

Use this skill for code reviews, PR reviews, security/quality passes, and
"is this good?" requests. Optimize for findings that can be acted on
immediately.

## Review Standard

- Report only issues that materially affect correctness, security,
  maintainability, portability, reliability, performance, or test confidence.
- Do not comment on style, formatting, naming, or preferences unless they hide a
  real bug or violate an explicit project rule.
- Prefer a small number of high-confidence findings over a long checklist.
- Verify each finding against the code path, configuration, tests, and platform
  assumptions before reporting it.
- Distinguish confirmed defects from risks, and do not inflate severity.
- If a tool, test, or search fails, say what coverage is missing instead of
  pretending the review is complete.

## What to Look For

- Logic bugs, wrong edge-case behavior, data loss, race conditions, and broken
  idempotency.
- Security issues: injection, path traversal, unsafe shell execution, secret
  exposure, weak trust boundaries, overbroad permissions, unsafe dependency or
  plugin loading.
- Error handling problems: swallowed errors, broad fallbacks, misleading success
  paths, lost context, or failures that become silent no-ops.
- Tests that do not cover changed behavior, regressions, platform-specific code,
  or important failure paths.
- Configuration drift: code, docs, schemas, CI, generated files, and validation
  commands disagreeing after a change.
- Cross-platform hazards in paths, symlinks, shells, permissions, line endings,
  process execution, Windows/Linux feature gates, or CI matrices.
- Dependency, build, or release risks introduced by changed manifests, lockfiles,
  workflow permissions, or artifact publishing.

## Review Process

1. Identify the changed files and intended behavior.
2. Read the relevant surrounding code and tests; do not review only the diff
   when context determines correctness.
3. Trace inputs to outputs for changed behavior, especially across config,
   filesystem, shell/process, network, auth, or persistence boundaries.
4. Check whether existing tests or validation commands actually exercise the
   behavior. Recommend targeted tests when coverage is missing.
5. Self-check every finding: "Can I point to exact evidence and a plausible
   failure mode?" If not, omit it or label it as a question.

## Output

- Start with the highest-impact finding, not a recap.
- For each finding include: severity, file/line, evidence, impact, and a
  minimal suggested fix.
- Use `blocking` only for issues that can realistically break users, leak data,
  corrupt state, weaken security, or fail CI/release.
- If there are no findings, say what was reviewed and that no material issues
  were found.
- Do not include generic praise, exhaustive checklists, or "looks good overall"
  filler.
