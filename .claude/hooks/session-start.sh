#!/bin/bash
# SessionStart hook: rebuild the RE environment in a fresh remote container.
#
# Runs scripts/setup_container.sh (idempotent: a warm container passes through
# in seconds) and persists the PATH/JAVA_HOME the pipeline needs. Registered
# BEFORE the `bd prime` hook in .claude/settings.json, because bd does not
# exist in a fresh container until this has run.
set -euo pipefail

# Local checkouts manage their own environment; this is for the ephemeral
# Claude Code remote container only.
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

sh "$CLAUDE_PROJECT_DIR/scripts/setup_container.sh"

# Tool locations for the rest of the session: bd lands in ~/go/bin, pact in
# ~/.cargo/bin, and scripts/ghidra/env.sh honours a preset JAVA_HOME.
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
  echo 'export PATH="$PATH:$HOME/go/bin:$HOME/.cargo/bin"' >> "$CLAUDE_ENV_FILE"
  if [ -d /usr/lib/jvm/java-21-openjdk-amd64 ]; then
    echo 'export JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64' >> "$CLAUDE_ENV_FILE"
  fi
fi
