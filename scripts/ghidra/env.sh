# Shared configuration for the Ghidra harness.  Sourced, not run.
#
# GHIDRA_HOME must point at a Ghidra 12.x install.  JAVA_HOME must be a real
# JDK: the launcher compiles the .java scripts, so a JRE fails with the
# unhelpful "Unable to prompt user for JDK path".
: "${GHIDRA_HOME:=tmp/ghidra_12.1.3_PUBLIC}"
: "${GHIDRA_PROJ:=tmp/ghidra_proj}"
: "${GHIDRA_IMAGE:=seeds/rally_f100.seed}"
: "${JAVA_HOME:=$(for d in "$HOME"/.local/share/mise/installs/java/*/; do
      [ -x "$d/bin/javac" ] && printf %s "${d%/}" && break; done)}"
export JAVA_HOME
PATH="$JAVA_HOME/bin:$PATH"; export PATH
HEADLESS="$GHIDRA_HOME/support/analyzeHeadless"
SCRIPTS="$(cd "$(dirname "$0")" && pwd)"
