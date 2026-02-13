#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# ai-pr.sh
# -----------------------------------------------------------------------------
# AI-powered PR creation using GitHub Copilot CLI
#
# Generates a pull request title and description from the diff and commit
# messages between the current branch and the default branch.
#
# Usage:
#   ai-pr.sh github [gh-pr-create-args...]
#   ai-pr.sh azure  [az-repos-pr-create-args...]
#
# Arguments:
#   $1  mode (github|azure) - determines which PR system to use
#   $@  additional arguments passed to gh or az CLI
#
# Prerequisites:
#   - gh CLI with copilot extension (required for github, optional for azure)
#   - az CLI (required for azure mode)
#
# Exit Codes:
#   0  PR created successfully or user cancelled
#   1  Error: missing prerequisites, invalid branch, or no changes
# -----------------------------------------------------------------------------

# Parse mode argument (github or azure)
MODE="${1:-}"
shift || true

# Validate mode was provided
if [ -z "$MODE" ]; then
  echo "Usage: ai-pr.sh <github|azure> [extra-args...]"
  exit 1
fi

# Check for required tools based on selected mode
case "$MODE" in
  github)
    # GitHub mode requires gh CLI
    if ! command -v gh >/dev/null 2>&1; then
      echo "Error: gh CLI not found. Install from https://cli.github.com/"
      exit 1
    fi
    ;;
  azure)
    # Azure mode requires az CLI
    if ! command -v az >/dev/null 2>&1; then
      echo "Error: Azure CLI (az) not found. Install from https://docs.microsoft.com/cli/azure/install-azure-cli"
      exit 1
    fi
    ;;
  *)
    # Invalid mode provided
    echo "Error: Unknown mode '$MODE'. Use 'github' or 'azure'"
    exit 1
    ;;
esac

# Get current branch name
CURRENT_BRANCH=$(git branch --show-current)
if [ -z "$CURRENT_BRANCH" ]; then
  echo "Error: Not on a branch"
  exit 1
fi

# Determine default branch (usually main or master)
# Query remote to find the HEAD branch
DEFAULT_BRANCH=$(git remote show origin | grep 'HEAD branch' | cut -d' ' -f5)
if [ -z "$DEFAULT_BRANCH" ]; then
  # Fallback to 'main' if remote query fails
  DEFAULT_BRANCH="main"
fi

# Prevent creating PR from the default branch itself
if [ "$CURRENT_BRANCH" = "$DEFAULT_BRANCH" ]; then
  echo "Error: Cannot create PR from default branch ($DEFAULT_BRANCH)"
  exit 1
fi

echo "Generating PR title and description..."

# Check if there are any changes between branches
# git diff --quiet returns:
#   0 if no differences
#   1 if there are differences
#   >1 if error occurred
git diff --quiet "$DEFAULT_BRANCH"...HEAD
DIFF_STATUS=$?
if [ "$DIFF_STATUS" -eq 0 ]; then
  echo "Error: No changes between $DEFAULT_BRANCH and $CURRENT_BRANCH"
  exit 1
elif [ "$DIFF_STATUS" -ne 1 ]; then
  echo "Error: Failed to compute diff between $DEFAULT_BRANCH and $CURRENT_BRANCH"
  exit 1
fi

# Collect commit messages and diff for AI generation
# Format: short hash + subject line
COMMITS=$(git log "$DEFAULT_BRANCH"..HEAD --pretty=format:'%h %s')
# Get full diff between branches (three-dot syntax for merge base)
DIFF=$(git diff "$DEFAULT_BRANCH"...HEAD)

# Check if GitHub Copilot is available for AI generation
# Copilot is optional for azure mode but enhances both modes
HAS_COPILOT=true
if ! command -v gh >/dev/null 2>&1; then
  HAS_COPILOT=false
fi

# Generate PR title and description
if [ "$HAS_COPILOT" = false ]; then
  # Fallback: use first commit message as title, list commits as body
  echo "Warning: gh CLI or copilot extension not found, using commit messages"
  PR_TITLE=$(echo "$COMMITS" | head -1 | cut -d' ' -f2-)
  PR_BODY="## Changes

$COMMITS"
else
  # Use GitHub Copilot to generate AI-powered PR title and description
  # Construct prompt with structured format requirements
  PROMPT="Based on these commits and diff, generate a pull request title (one line) and description (multiple paragraphs with markdown formatting). Format your response EXACTLY as:

TITLE: <title here>

DESCRIPTION:
<description here>

Commits:
$COMMITS

Diff:
$DIFF"
  # Send prompt to Copilot and capture response
  # Suppress stderr to avoid progress messages
  AI_RESPONSE=$(printf '%b' "$PROMPT" | gh copilot -p "" 2>/dev/null) || true

  # Parse AI response or fallback to commit messages
  if [ -z "$AI_RESPONSE" ]; then
    # AI generation failed, use fallback
    PR_TITLE=$(echo "$COMMITS" | head -1 | cut -d' ' -f2-)
    PR_BODY="## Changes

$COMMITS"
  else
    # Extract title and description from AI response
    # Title is prefixed with "TITLE:", description is between "DESCRIPTION:" and "Diff:"
    PR_TITLE=$(echo "$AI_RESPONSE" | grep '^TITLE:' | head -1 | sed 's/^TITLE:[[:space:]]*//')
    PR_BODY=$(echo "$AI_RESPONSE" | sed -n '/^DESCRIPTION:/,/^Diff:/p' | sed '1d;$d')

    # Validate extraction succeeded, fallback if empty
    if [ -z "$PR_TITLE" ]; then
      PR_TITLE=$(echo "$COMMITS" | head -1 | cut -d' ' -f2-)
    fi
    if [ -z "$PR_BODY" ]; then
      PR_BODY="## Changes

$COMMITS"
    fi
  fi
fi

# Display generated PR content and prompt for confirmation
echo ""
echo "=== Generated PR ==="
echo "Title: $PR_TITLE"
echo ""
echo "Description:"
echo "$PR_BODY"
echo "==================="
echo ""

# Ask user to confirm before creating PR
printf "Create PR with this content? [y/N] "
IFS= read -r ans

# Process user response
case "$ans" in
  [Yy]*)
    # User confirmed, create PR using selected mode
    case "$MODE" in
      github)
        # Create GitHub PR using gh CLI
        # Pass through any additional arguments from command line
        gh pr create --title "$PR_TITLE" --body "$PR_BODY" "$@"
        ;;
      azure)
        # Create Azure DevOps PR using az CLI
        # Requires source and target branches to be explicitly specified
        az repos pr create --title "$PR_TITLE" --description "$PR_BODY" \
          --source-branch "$CURRENT_BRANCH" --target-branch "$DEFAULT_BRANCH" "$@"
        ;;
    esac
    ;;
  *)
    # User declined or entered anything other than y/Y
    echo "PR creation cancelled"
    ;;
esac
