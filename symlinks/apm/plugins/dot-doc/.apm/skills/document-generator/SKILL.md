---
name: document-generator
description: Use when creating Markdown documents from ideas, investigations, proposals, plans, reviews, or loose requirements. Prefer concise, evidence-based, actionable writing.
---

# Document Generator

Use this skill for document-like artifacts such as proposals, plans, investigations, recommendations, retrospectives, reviews, and design notes.

## Preferences

- Write concise GitHub-flavored Markdown with clear headings.
- Lead with the useful answer, recommendation, or decision.
- Ground the content in repository evidence and existing project conventions.
- Keep prose direct, practical, and engineering-focused; avoid corporate filler and generic advice.
- Prefer tables for comparisons, tradeoffs, risks, decisions, and option matrices.
- Use only the sections that fit the artifact; do not force a template.
- Separate facts from recommendations, and preserve uncertainty with "Unknown" or "Needs confirmation" instead of guessing.
- Include concrete next steps only when the document implies follow-up work.

## File Handling

- If the user provides an output path, write the document there.
- If no path is provided, use the conventional repository location when it is obvious; otherwise ask before writing.
- Do not create scratch Markdown files in the repository.
- If Word output is requested, create or update the Markdown source first, then use the available Markdown-to-DOCX workflow.
