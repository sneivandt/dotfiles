---
name: audit
description: >
  Investigate a subject in depth for defects, gaps, and concrete improvements.
  Use when the user says "audit" or asks for a systematic assessment of what
  is wrong, missing, or worth changing in a system, process, document, or other subject.
---

# Audit

Use the scope the user names. For a standalone `audit`, use the subject under
discussion or the active work if it provides a clear subject. Ask what to audit
if neither does.

## Investigate

- Establish what the subject is meant to achieve and which requirements or
  criteria apply. Identify the claims, assumptions, and dependencies that matter
  most to the outcome. Use them to decide where to dig and to check coverage.
- Trace consequential claims to their sources and examine how the subject holds
  up in representative cases and relevant exceptions. Follow each issue far
  enough to establish its cause, who or what it affects, and where its effects
  stop. Check what the available evidence actually covers.
- Compare intended and observed outcomes. For an improvement, trace a real
  scenario where the current approach costs time, causes confusion, or adds risk.
  Explain what a change would improve and what it would cost.
- For each serious candidate, look for evidence that could rule it out, such as
  an exception, an upstream safeguard, or a more authoritative source. Use
  scoped checks or isolated examples where practical. Carry remaining uncertainty
  into the report. Audits are read-only by default. Make edits or take external
  actions when separately authorized.

## Report

Lead with the most consequential findings. For each, cite the relevant source,
explain the triggering conditions and impact, and suggest a concrete change.
Distinguish verified defects, risks inferred from the evidence, and optional
improvements. Consolidate symptoms that share a root cause. Summarize the
evidence examined, checks run, and material gaps in coverage. Say plainly when
the audit finds no actionable issues.
