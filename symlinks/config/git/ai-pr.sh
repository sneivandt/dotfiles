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
# Use local ref to avoid slow network call to remote
DEFAULT_BRANCH=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's@^refs/remotes/origin/@@')
if [ -z "$DEFAULT_BRANCH" ]; then
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
git diff --quiet "$DEFAULT_BRANCH"...HEAD && DIFF_STATUS=0 || DIFF_STATUS=$?
if [ "$DIFF_STATUS" -eq 0 ]; then
  echo "Error: No changes between $DEFAULT_BRANCH and $CURRENT_BRANCH"
  exit 1
elif [ "$DIFF_STATUS" -ne 1 ]; then
  echo "Error: Failed to compute diff between $DEFAULT_BRANCH and $CURRENT_BRANCH"
  exit 1
fi

# Collect commit messages and diff for AI generation
COMMITS=$(git log "$DEFAULT_BRANCH"..HEAD --pretty=format:'%h %s')
# Get diff stat summary and the actual diff (truncated to avoid prompt limits)
DIFF_STAT=$(git diff "$DEFAULT_BRANCH"...HEAD --stat)
DIFF=$(git diff "$DEFAULT_BRANCH"...HEAD | head -500)
# List changed files for additional context
CHANGED_FILES=$(git diff "$DEFAULT_BRANCH"...HEAD --name-only)

# Check if GitHub Copilot CLI is available
HAS_COPILOT=true
if ! command -v gh >/dev/null 2>&1; then
  HAS_COPILOT=false
fi

# Generate PR title and description
if [ "$HAS_COPILOT" = false ]; then
  echo "Warning: gh CLI not found, using commit messages as fallback"
  PR_TITLE=$(echo "$COMMITS" | head -1 | cut -d' ' -f2-)
  PR_BODY="## Changes

$COMMITS"
else
  # Build a prompt focused on the actual code changes
  PROMPT="You are generating a pull request title and description. Analyze the code diff below and describe WHAT changed and WHY based on the actual modifications, not the commit messages.

Format your response EXACTLY as (no extra text before TITLE):
TITLE: <a concise descriptive title summarizing the change>

DESCRIPTION:
<markdown description with sections like ## Summary, ## Changes, ## Impact as appropriate>

Files changed:
$CHANGED_FILES

Diff stats:
$DIFF_STAT

Code diff:
$DIFF"

  # Write prompt to a temp file to avoid shell argument length limits
  PROMPT_FILE=$(mktemp)
  printf '%s' "$PROMPT" > "$PROMPT_FILE"

  # Setup cleanup trap to restore MCP config on any exit (success or failure)
  MCP_USER_CONFIG="$HOME/.copilot/mcp-config.json"
  MCP_BACKUP=""
  cleanup_mcp_config() {
    if [ -n "$MCP_BACKUP" ] && [ -f "$MCP_BACKUP" ]; then
      mv "$MCP_BACKUP" "$MCP_USER_CONFIG" 2>/dev/null || true
    fi
    rm -f "$PROMPT_FILE" 2>/dev/null || true
  }
  trap cleanup_mcp_config EXIT

  # Temporarily hide MCP config so Copilot CLI doesn't start MCP servers (slow)
  if [ -f "$MCP_USER_CONFIG" ]; then
    MCP_BACKUP="$MCP_USER_CONFIG.bak"
    mv "$MCP_USER_CONFIG" "$MCP_BACKUP"
  fi

  # Use gh copilot CLI — ask it to read and respond to the prompt file
  AI_RESPONSE=$(gh copilot -- -p "Read the file $PROMPT_FILE and follow the instructions in it exactly. Output ONLY what the instructions ask for." \
    --disable-builtin-mcps \
    --allow-all-tools \
    2>/dev/null) || true

  # Strip Copilot CLI usage stats from the response
  AI_RESPONSE=$(echo "$AI_RESPONSE" | sed '/^● /d' | sed '/^Total usage/,$d')

  # Parse AI response or fallback to commit messages
  if [ -z "$AI_RESPONSE" ]; then
    PR_TITLE=$(echo "$COMMITS" | head -1 | cut -d' ' -f2-)
    PR_BODY="## Changes

$COMMITS"
  else
    PR_TITLE=$(echo "$AI_RESPONSE" | grep '^TITLE:' | head -1 | sed 's/^TITLE:[[:space:]]*//')
    # Extract everything after DESCRIPTION: line
    PR_BODY=$(echo "$AI_RESPONSE" | sed -n '/^DESCRIPTION:/,$p' | sed '1d')

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
    # Push branch to remote if not already there
    if ! git ls-remote --exit-code origin "refs/heads/$CURRENT_BRANCH" >/dev/null 2>&1; then
      echo "Pushing branch '$CURRENT_BRANCH' to origin..."
      git push -u origin "$CURRENT_BRANCH"
    fi
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
