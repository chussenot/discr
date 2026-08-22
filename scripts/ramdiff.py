#!/usr/bin/env python3
"""
ramdiff.py - narrow down game-state addresses from Hatari RAM dumps.

Workflow (Cheat Engine style narrowing):
  1. Take several 1 MB dumps in different, *known* game situations.
  2. Describe what you expect between consecutive dumps:
       changed   - the value must differ   (e.g. player moved right)
       same      - the value must be equal (e.g. player did NOT move)
       increased - value grew   (interpret as big-endian u16 at even addrs)
       decreased - value shrank
  3. The script intersects all constraints and prints surviving addresses.

Examples:
  # Player X hunt: stand still, dump A; move right, dump B; move right, dump C;
  # stand still, dump D.
  ./ramdiff.py --word \
      changed   a.bin b.bin \
      increased b.bin c.bin \
      same      c.bin d.bin

  # Tile grid hunt (bytes, not words): destroy one tile between A and B,
  # nothing else:
  ./ramdiff.py --byte changed a.bin b.bin same b.bin b2.bin

Options:
  --byte / --word     granularity (default: word = big-endian u16, 68000-friendly)
  --range START END   hex range to consider (default 0x0 0x100000; use it to
                      exclude the framebuffer once you know its address)
  --max N             stop printing after N results (default 200)
"""

import argparse
import struct
import sys


def load(path: str) -> bytes:
    with open(path, "rb") as f:
        return f.read()


def values(buf: bytes, addr: int, word: bool) -> int:
    if word:
        return struct.unpack_from(">H", buf, addr)[0]
    return buf[addr]


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("constraints", nargs="+",
                   help="triplets: {changed|same|increased|decreased} fileA fileB ...")
    p.add_argument("--byte", action="store_true", help="byte granularity")
    p.add_argument("--word", action="store_true", help="word granularity (default)")
    p.add_argument("--range", nargs=2, default=["0x0", "0x100000"],
                   metavar=("START", "END"), help="hex address range")
    p.add_argument("--max", type=int, default=200, help="max results to print")
    args = p.parse_args()

    if len(args.constraints) % 3 != 0:
        sys.exit("constraints must be triplets: OP fileA fileB")

    word = not args.byte
    step = 2 if word else 1
    start = int(args.range[0], 16)
    end = int(args.range[1], 16)
    if word and start % 2:
        start += 1  # 68000 words are even-aligned

    triplets = [args.constraints[i:i + 3]
                for i in range(0, len(args.constraints), 3)]

    cache: dict[str, bytes] = {}
    for _, fa, fb in triplets:
        for f in (fa, fb):
            if f not in cache:
                cache[f] = load(f)

    sizes = {len(b) for b in cache.values()}
    if len(sizes) != 1:
        sys.exit(f"dump sizes differ: {sizes} - always dump the same range")
    size = min(sizes.pop(), end)

    ops = {
        "changed":   lambda a, b: a != b,
        "same":      lambda a, b: a == b,
        "increased": lambda a, b: b > a,
        "decreased": lambda a, b: b < a,
    }

    survivors = []
    for addr in range(start, size - step + 1, step):
        ok = True
        for op, fa, fb in triplets:
            va = values(cache[fa], addr, word)
            vb = values(cache[fb], addr, word)
            if not ops[op](va, vb):
                ok = False
                break
        if ok:
            survivors.append(addr)
            if len(survivors) > args.max * 4:
                break  # runaway: constraints too weak

    print(f"{len(survivors)} candidate address(es)")
    last_fa, last_fb = triplets[-1][1], triplets[-1][2]
    for addr in survivors[:args.max]:
        va = values(cache[last_fa], addr, word)
        vb = values(cache[last_fb], addr, word)
        print(f"  ${addr:06x}  {va:6d} -> {vb:6d}   (0x{va:04x} -> 0x{vb:04x})")
    if len(survivors) > args.max:
        print(f"  ... {len(survivors) - args.max} more (add constraints to narrow)")


if __name__ == "__main__":
    main()
