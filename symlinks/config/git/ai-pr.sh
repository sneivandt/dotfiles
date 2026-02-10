#!/bin/sh
# AI-powered PR creation using GitHub Copilot CLI
#
# Generates a pull request title and description from the diff and commit
# messages between the current branch and the default branch.
#
# Usage:
#   ai-pr.sh github [gh-pr-create-args...]
#   ai-pr.sh azure  [az-repos-pr-create-args...]
#
# Prerequisites:
#   - gh CLI with copilot extension (required for github, optional for azure)
#   - az CLI (required for azure mode)

set -o errexit
set -o nounset

MODE="${1:-}"
shift || true

if [ -z "$MODE" ]; then
    echo "Usage: ai-pr.sh <github|azure> [extra-args...]"
    exit 1
fi

case "$MODE" in
    github)
        if ! command -v gh >/dev/null 2>&1; then
            echo "Error: gh CLI not found. Install from https://cli.github.com/"
            exit 1
        fi
        if ! gh copilot --version >/dev/null 2>&1; then
            echo "Error: gh copilot extension not found. Install with: gh extension install github/gh-copilot"
            exit 1
        fi
        ;;
    azure)
        if ! command -v az >/dev/null 2>&1; then
            echo "Error: Azure CLI (az) not found. Install from https://docs.microsoft.com/cli/azure/install-azure-cli"
            exit 1
        fi
        ;;
    *)
        echo "Error: Unknown mode '$MODE'. Use 'github' or 'azure'"
        exit 1
        ;;
esac

CURRENT_BRANCH=$(git branch --show-current)
if [ -z "$CURRENT_BRANCH" ]; then
    echo "Error: Not on a branch"
    exit 1
fi

DEFAULT_BRANCH=$(git remote show origin | grep 'HEAD branch' | cut -d' ' -f5)
if [ -z "$DEFAULT_BRANCH" ]; then
    DEFAULT_BRANCH="main"
fi

if [ "$CURRENT_BRANCH" = "$DEFAULT_BRANCH" ]; then
    echo "Error: Cannot create PR from default branch ($DEFAULT_BRANCH)"
    exit 1
fi

echo "Generating PR title and description..."

git diff --quiet "$DEFAULT_BRANCH"...HEAD
DIFF_STATUS=$?
if [ "$DIFF_STATUS" -eq 0 ]; then
    echo "Error: No changes between $DEFAULT_BRANCH and $CURRENT_BRANCH"
    exit 1
elif [ "$DIFF_STATUS" -ne 1 ]; then
    echo "Error: Failed to compute diff between $DEFAULT_BRANCH and $CURRENT_BRANCH"
    exit 1
fi

COMMITS=$(git log "$DEFAULT_BRANCH"..HEAD --pretty=format:'%h %s')
DIFF=$(git diff "$DEFAULT_BRANCH"...HEAD)

HAS_COPILOT=true
if ! command -v gh >/dev/null 2>&1 || ! gh copilot --version >/dev/null 2>&1; then
    HAS_COPILOT=false
fi

if [ "$HAS_COPILOT" = false ]; then
    echo "Warning: gh CLI or copilot extension not found, using commit messages"
    PR_TITLE=$(echo "$COMMITS" | head -1 | cut -d' ' -f2-)
    PR_BODY="## Changes

$COMMITS"
else
    PROMPT="Based on these commits and diff, generate a pull request title (one line) and description (multiple paragraphs with markdown formatting). Format your response EXACTLY as:

TITLE: <title here>

DESCRIPTION:
<description here>

Commits:
$COMMITS

Diff:
$DIFF"
    AI_RESPONSE=$(printf '%b' "$PROMPT" | gh copilot -p "" 2>/dev/null) || true
    if [ -z "$AI_RESPONSE" ]; then
        PR_TITLE=$(echo "$COMMITS" | head -1 | cut -d' ' -f2-)
        PR_BODY="## Changes

$COMMITS"
    else
        PR_TITLE=$(echo "$AI_RESPONSE" | grep '^TITLE:' | head -1 | sed 's/^TITLE:[[:space:]]*//')
        PR_BODY=$(echo "$AI_RESPONSE" | sed -n '/^DESCRIPTION:/,/^Diff:/p' | sed '1d;$d')
        if [ -z "$PR_TITLE" ]; then
            PR_TITLE=$(echo "$COMMITS" | head -1 | cut -d' ' -f2-)
        fi
        if [ -z "$PR_BODY" ]; then
            PR_BODY="## Changes

$COMMITS"
        fi
    fi
fi

echo ""
echo "=== Generated PR ==="
echo "Title: $PR_TITLE"
echo ""
echo "Description:"
echo "$PR_BODY"
echo "==================="
echo ""
printf "Create PR with this content? [y/N] "
IFS= read -r ans
case "$ans" in
    [Yy]*)
        case "$MODE" in
            github) gh pr create --title "$PR_TITLE" --body "$PR_BODY" "$@" ;;
            azure) az repos pr create --title "$PR_TITLE" --description "$PR_BODY" \
                --source-branch "$CURRENT_BRANCH" --target-branch "$DEFAULT_BRANCH" "$@" ;;
        esac
        ;;
    *) echo "PR creation cancelled" ;;
esac
