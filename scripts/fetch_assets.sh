#!/bin/sh
# Fetch the two gitignored emulator inputs into tmp/:
#
#   tmp/Disc (1990)(Loriciel)[cr Exo-7].st     the game disk image
#   tmp/emutos-512k-1.4/etos512us.img          the TOS ROM (EmuTOS 1.4, 512k)
#
# These are what scripts/collect.py (and everything built on it: oracle_diff,
# seed_relay, oracle_check) needs to drive Hatari. Neither may ever be
# committed -- tmp/, *.st, *.img and *.zip are all gitignored, which is why
# they live under tmp/.
#
# Sources:
#   * the game: its abandonware page (DISC_PAGE, default
#     https://www.myabandonware.com/game/disc-vp), scraped for the download
#     link. If the scrape breaks, pass DISC_URL=<direct link to the .zip or
#     .st>, or download the zip by hand into tmp/ and re-run -- this script
#     will find and unpack it.
#   * the TOS ROM: EmuTOS's SourceForge releases (free software, so a stable
#     direct link). EmuTOS is GPL: source at https://github.com/emutos/emutos,
#     and the 1.4 source/binary packages at
#     https://sourceforge.net/projects/emutos/files/emutos/1.4/
#
# Re-running is a no-op once both files exist; FORCE=1 re-downloads.
set -eu

cd "$(dirname "$0")/.."

TMP=tmp
DISK_NAME="Disc (1990)(Loriciel)[cr Exo-7].st"
DISK="$TMP/$DISK_NAME"
TOS="$TMP/emutos-512k-1.4/etos512us.img"

DISC_PAGE=${DISC_PAGE:-"https://www.myabandonware.com/game/disc-vp"}
# The master mirror is pinned because the generic downloads.sourceforge.net
# redirector picks a random mirror host, some of which reject or stall.
EMUTOS_URL=${EMUTOS_URL:-"https://master.dl.sourceforge.net/project/emutos/emutos/1.4/emutos-512k-1.4.zip?viasf=1"}
EMUTOS_URL_FALLBACK="https://downloads.sourceforge.net/project/emutos/emutos/1.4/emutos-512k-1.4.zip"
FORCE=${FORCE:-0}

# The abandonware site refuses the default curl/wget agent string, so those
# fetches masquerade as a browser. SourceForge is the opposite: a browser
# agent gets bounced to an interstitial page instead of the file, so the
# EmuTOS fetch keeps the default agent.
UA="Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0"

have() { command -v "$1" >/dev/null 2>&1; }

# fetch <url> <outfile>
fetch() {
    if have curl; then
        curl -fSL --retry 3 --connect-timeout 30 -o "$2" "$1"
    elif have wget; then
        wget -q -O "$2" "$1"
    else
        echo "fetch_assets: need curl or wget" >&2
        exit 1
    fi
}

# fetch_browser <url> <outfile> [referer]
fetch_browser() {
    if have curl; then
        curl -fSL --retry 3 --connect-timeout 30 -A "$UA" \
             ${3:+-e "$3"} -o "$2" "$1"
    elif have wget; then
        wget -q -U "$UA" ${3:+--referer="$3"} -O "$2" "$1"
    else
        echo "fetch_assets: need curl or wget" >&2
        exit 1
    fi
}

# Every quoted attribute value (hrefs included) of an HTML file, one per line,
# with &amp; undone -- crude, POSIX, and enough to find a download link.
urls_in() { tr '"' '\n' <"$1" | sed 's/&amp;/\&/g'; }

is_zip() { [ "$(dd if="$1" bs=2 count=1 2>/dev/null)" = "PK" ]; }

checksum() {
    if have sha256sum; then sha256sum "$1"
    elif have shasum; then shasum -a 256 "$1"
    fi
}

manual_help() {
    cat >&2 <<EOF

fetch_assets: could not fetch the game automatically. Get it by hand:

  1. open $DISC_PAGE
  2. download the Atari ST release ("$DISK_NAME" zipped)
  3. either save the zip anywhere under $TMP/ and re-run this script,
     or unpack it yourself so that "$DISK" exists.

A direct link (to the .zip or the .st) also works:
  DISC_URL='https://...' $0
EOF
    exit 1
}

mkdir -p "$TMP"

# ---- TOS ROM: EmuTOS 1.4 (512k) ----------------------------------------
if [ -f "$TOS" ] && [ "$FORCE" != 1 ]; then
    echo "fetch_assets: TOS ROM already present: $TOS"
else
    have unzip || { echo "fetch_assets: need unzip" >&2; exit 1; }
    echo "fetch_assets: downloading EmuTOS 1.4 (512k)"
    fetch "$EMUTOS_URL" "$TMP/emutos-512k-1.4.zip" \
        || fetch "$EMUTOS_URL_FALLBACK" "$TMP/emutos-512k-1.4.zip"
    unzip -oq "$TMP/emutos-512k-1.4.zip" -d "$TMP"
    [ -f "$TOS" ] || {
        echo "fetch_assets: $EMUTOS_URL did not contain $TOS" >&2; exit 1; }
    echo "fetch_assets: installed $TOS"
fi

# ---- game disk image ----------------------------------------------------
if [ -f "$DISK" ] && [ "$FORCE" != 1 ]; then
    echo "fetch_assets: disk image already present: $DISK"
else
    work="$TMP/.fetch-disc"
    rm -rf "$work"
    mkdir -p "$work"
    payload="$work/payload"

    if [ -n "${DISC_URL:-}" ]; then
        echo "fetch_assets: downloading game from DISC_URL"
        fetch_browser "$DISC_URL" "$payload" || manual_help
    else
        # A zip dropped into tmp/ by hand wins over the network.
        for f in "$TMP"/*.zip; do
            if [ -f "$f" ] && [ "$f" != "$TMP/emutos-512k-1.4.zip" ] \
               && unzip -l "$f" 2>/dev/null | grep -qi '\.st$'; then
                echo "fetch_assets: using already-downloaded $f"
                cp -f "$f" "$payload"
                break
            fi
        done
        if [ ! -f "$payload" ]; then
            echo "fetch_assets: scraping $DISC_PAGE"
            fetch_browser "$DISC_PAGE" "$work/game.html" || manual_help
            dlpage=$(urls_in "$work/game.html" | grep '/download/' | head -n 1 || true)
            case "$dlpage" in
                https://*|http://*) ;;
                /*) dlpage="https://www.myabandonware.com$dlpage" ;;
                *)  dlpage="" ;;
            esac
            [ -n "$dlpage" ] || manual_help
            echo "fetch_assets: download page $dlpage"
            fetch_browser "$dlpage" "$work/dl.html" "$DISC_PAGE" || manual_help
            fileurl=$(urls_in "$work/dl.html" \
                      | grep -E '^https?://[^ ]*\.([Zz][Ii][Pp]|[Ss][Tt])(\?[^ ]*)?$' \
                      | head -n 1 || true)
            [ -n "$fileurl" ] || fileurl=$(urls_in "$work/dl.html" \
                      | grep -E '^https?://download\.' | head -n 1 || true)
            [ -n "$fileurl" ] || manual_help
            echo "fetch_assets: downloading $fileurl"
            fetch_browser "$fileurl" "$payload" "$dlpage" || manual_help
        fi
    fi

    if is_zip "$payload"; then
        have unzip || { echo "fetch_assets: need unzip" >&2; exit 1; }
        unzip -oq "$payload" -d "$work/unpacked"
        if [ -f "$work/unpacked/$DISK_NAME" ]; then
            st="$work/unpacked/$DISK_NAME"
        else
            st=$(find "$work/unpacked" -type f \
                 \( -name '*.st' -o -name '*.ST' \) | head -n 1)
        fi
        [ -n "$st" ] || { echo "fetch_assets: archive held no .st image" >&2
                          manual_help; }
        if [ "$(basename "$st")" != "$DISK_NAME" ]; then
            echo "fetch_assets: WARNING: dump is named '$(basename "$st")'," \
                 "not '$DISK_NAME'; installing it under the expected name," \
                 "but a different dump may not match the documented addresses" >&2
        fi
        mv -f "$st" "$DISK"
    else
        mv -f "$payload" "$DISK"
    fi
    rm -rf "$work"
    echo "fetch_assets: installed $DISK"
fi

echo
echo "fetch_assets: done."
checksum "$DISK" || true
checksum "$TOS" || true
