#!/bin/sh
# Rebuild the full reverse-engineering environment from a fresh clone.
#
# Everything this repo's pipeline needs beyond a Rust toolchain, in one
# idempotent pass -- safe to re-run, each component is skipped when already
# present. Written for the Claude Code remote container (Ubuntu, root, apt),
# where the container is ephemeral and this script is what makes a new one
# usable; it works on any Debian-family box with sudo.
#
#   sh scripts/setup_container.sh          # everything
#   SKIP_GHIDRA=1 sh scripts/setup_container.sh   # skip the 543 MB download
#
# What it installs, and why:
#   apt packages   oracle build (libssl-dev), Hatari build (cmake, SDL2, png,
#                  readline, zlib), headless X (xvfb, python3-xlib -- XTEST
#                  joystick injection, see scripts/collect.py), disc-app
#                  (libGL/libX11/libasound/libXi), Ghidra (openjdk-21), unzip
#   Hatari 2.6.1   built from source. The Ubuntu package is 2.4.1, which lacks
#                  --screenshot-format and differs in the debugger quirks
#                  KNOWN_ISSUES.md documents against 2.6.1 exactly.
#   Ghidra 12.1.3  into tmp/ghidra_12.1.3_PUBLIC, where scripts/ghidra/env.sh
#                  expects it. sha256-pinned.
#   dolt, bd, pact the issue tracker (beads over a dolt sql-server; the repo's
#                  .beads/metadata.json is in server mode) and the fleet
#                  coordination CLI.
#   oracle         make -C oracle, so disc-oracle is ready for seeds.
#
# What it does NOT do: mint seeds (scenarios/oracle_seed.yaml does, against a
# live Hatari), import into Ghidra (scripts/ghidra/import.sh), or set a pact
# agent identity (PACT_AGENT is per-agent by design -- pact never guesses).
set -eu

cd "$(dirname "$0")/.."

HATARI_VERSION=2.6.1
GHIDRA_DIR=tmp/ghidra_12.1.3_PUBLIC
GHIDRA_URL="https://github.com/NationalSecurityAgency/ghidra/releases/download/Ghidra_12.1.3_build/ghidra_12.1.3_PUBLIC_20260817.zip"
GHIDRA_SHA256=93a5d11a9ad510622acaaf908c556a7b9b764d338e78a7567f3689bf5081fd54
JAVA_HOME_DEFAULT=/usr/lib/jvm/java-21-openjdk-amd64

have() { command -v "$1" >/dev/null 2>&1; }
as_root() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi; }
say() { echo "setup_container: $*"; }

# ---- apt packages --------------------------------------------------------
PKGS="libssl-dev cmake libsdl2-dev zlib1g-dev libpng-dev libreadline-dev \
      xvfb python3-xlib libgl1-mesa-dev libx11-dev libasound2-dev libxi-dev \
      openjdk-21-jdk unzip make gcc"
missing=""
for p in $PKGS; do
    dpkg -s "$p" >/dev/null 2>&1 || missing="$missing $p"
done
if [ -n "$missing" ]; then
    say "apt: installing$missing"
    # Best-effort update first: stale mirrors 404 otherwise. Dead third-party
    # PPAs in the image may fail the update; the install is what matters.
    as_root apt-get update >/dev/null 2>&1 || true
    as_root apt-get install -y $missing >/dev/null
else
    say "apt: all packages present"
fi

# ---- Hatari 2.6.1 from source -------------------------------------------
if hatari -v 2>&1 | grep -q "v$HATARI_VERSION"; then
    say "hatari: $HATARI_VERSION already installed"
else
    say "hatari: building $HATARI_VERSION from source"
    # The distro package (2.4.1 on noble) must not shadow the build.
    if dpkg -s hatari >/dev/null 2>&1; then as_root apt-get remove -y hatari >/dev/null; fi
    build=$(mktemp -d)
    GIT_LFS_SKIP_SMUDGE=1 git clone -q --depth 1 --branch "v$HATARI_VERSION" \
        https://github.com/hatari/hatari.git "$build/hatari"
    cmake -S "$build/hatari" -B "$build/hatari/build" \
        -DCMAKE_BUILD_TYPE=Release >/dev/null
    make -C "$build/hatari/build" -j"$(nproc)" >/dev/null
    as_root make -C "$build/hatari/build" install >/dev/null
    rm -rf "$build"
    hatari -v 2>&1 | grep -q "v$HATARI_VERSION" || {
        say "hatari: build did not produce $HATARI_VERSION"; exit 1; }
    say "hatari: installed $(command -v hatari)"
fi

# ---- Ghidra 12.1.3 into tmp/ ---------------------------------------------
if [ -x "$GHIDRA_DIR/support/analyzeHeadless" ]; then
    say "ghidra: already at $GHIDRA_DIR"
elif [ "${SKIP_GHIDRA:-0}" = 1 ]; then
    say "ghidra: skipped (SKIP_GHIDRA=1)"
else
    say "ghidra: downloading 12.1.3 (543 MB)"
    mkdir -p tmp
    curl -fSL --retry 3 -o tmp/ghidra.zip "$GHIDRA_URL"
    echo "$GHIDRA_SHA256  tmp/ghidra.zip" | sha256sum -c - >/dev/null
    unzip -q tmp/ghidra.zip -d tmp
    rm -f tmp/ghidra.zip
    say "ghidra: installed at $GHIDRA_DIR"
fi
# scripts/ghidra/env.sh honours a preset JAVA_HOME and otherwise looks in
# mise's install dir, which the container does not have.
[ -d "$JAVA_HOME_DEFAULT" ] && say "ghidra: use JAVA_HOME=$JAVA_HOME_DEFAULT"

# ---- dolt, bd (beads), pact ----------------------------------------------
if have dolt; then
    say "dolt: $(dolt version | head -1)"
else
    say "dolt: installing"
    curl -fsSL https://github.com/dolthub/dolt/releases/latest/download/install.sh \
        | as_root bash >/dev/null 2>&1
fi

export PATH="$PATH:$HOME/go/bin:$HOME/.cargo/bin"
if have bd; then
    say "bd: $(bd version 2>/dev/null | head -1)"
else
    say "bd: installing (beads)"
    curl -fsSL https://raw.githubusercontent.com/steveyegge/beads/main/scripts/install.sh \
        | bash >/dev/null 2>&1 || true
    have bd || { say "bd: install failed -- install Go and re-run"; exit 1; }
fi

if have pact; then
    say "pact: $(pact --version 2>/dev/null | head -1)"
elif have cargo; then
    say "pact: building from chussenot/pact"
    cargo install -q --git https://github.com/chussenot/pact --locked pact 2>/dev/null \
        || cargo install -q --git https://github.com/chussenot/pact --locked
else
    say "pact: cargo not found, skipped"
fi

# ---- beads database (dolt server mode) ------------------------------------
# bd bootstrap hydrates from refs/dolt/data on the git origin when present.
# NOTE: in the remote container the git proxy accepts pushes to branch heads
# only, so `bd dolt pull` works but `bd dolt push` cannot -- hand bead changes
# back via `bd export -o .beads/issues.jsonl` committed on the branch.
if have bd; then
    if bd list >/dev/null 2>&1; then
        say "beads: database up"
        bd dolt pull >/dev/null 2>&1 && say "beads: pulled refs/dolt/data" \
            || say "beads: pull skipped (conflict or no remote data -- see bd dolt)"
    else
        say "beads: bootstrapping"
        bd bootstrap --yes >/dev/null 2>&1 && say "beads: bootstrapped" \
            || say "beads: bootstrap failed -- run bd bootstrap by hand"
    fi
fi

# ---- oracle ---------------------------------------------------------------
if make -C oracle >/dev/null 2>&1; then
    say "oracle: built ($(ls -l oracle/disc-oracle | awk '{print $5}') bytes)"
else
    say "oracle: build failed -- run make -C oracle to see why"
fi

say "done. Remaining by hand, when needed:"
say "  seeds:  python3 scripts/collect.py --scenario scenarios/oracle_seed.yaml"
say "  ghidra: JAVA_HOME=$JAVA_HOME_DEFAULT GHIDRA_IMAGE=seeds/<seed> ./scripts/ghidra/import.sh"
say "  gates:  cargo test && tracecheck runs (see mise.toml [tasks.core-check])"
