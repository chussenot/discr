//! Floor grid damage and destruction. Owned by bead `discr-qcb` (I4).
//!
//! ST `$7616`, 17 cells of stride 8, `{+$00 type, +$02 hp}`.
//!
//! ```text
//! $a31c  sub.w  ($0016,a5),d6      ; HP -= the striking disc's damage (+$16)
//! $a34a  clr.w  d6                 ; clamped at 0, never negative
//! $a34c  move.w d6,($02,a0,d5.w)   ; the HP store
//! $a354  clr.w  ($00,a0,d5.w)      ; HP == 0 also clears the TYPE word
//! $a360  move.b #$03,$6c5c         ; and queues the destruction sample
//! ```
//!
//! The `+$00` type word is the walkability gate the movement code `tst.w`s;
//! [`Tile::walkable`] is that predicate.
//!
//! # A bank is eight tiles held twice (Part 10e)
//!
//! `$a3a6 lea $7656,a0; adda.w d5,a0` is the instruction that explains the
//! whole layout. `$7656` is `$7616 + 8 * 8`, and `d5` is the struck cell's byte
//! offset, so the destroy path takes the cell **eight further on**. Put that
//! beside the two index formulas and it closes:
//!
//! ```text
//! a disc's cell   ($a250)  = column(world_x + 4) + (4 if world_y > $46)   1..8
//! a player's cell ($f836)  = 8 + column(world_x)  + (4 if world_y > 14)   9..16
//! ```
//!
//! **Cells 1..8 and 9..16 are the same eight tiles**: 1..8 is the record the
//! disc's damage path writes, 9..16 the copy the movement code reads. That is
//! why the eight low cells carry hp 4 or 5 in a fresh round and the eight high
//! ones all carry a dummy hp of 1 -- the high copy only ever needs a type.
//!
//! And the two are **not** kept in step. Destroying a low cell starts a
//! collapse animation, and the high copy's type is cleared only when that
//! animation finishes, 49 ticks later. See [`Collapse`].

use crate::{Event, TILE_CELLS, TILE_TYPE_DESTROYED, Tile};

/// How far on the movement code's copy of a tile is. ST `$a3a6`: `$7656` is
/// `$7616 + 8 * 8`, so eight cells.
pub const WALK_COPY_OFFSET: usize = 8;

/// Frames a destroyed tile takes to collapse. ST: the length of the sprite
/// frame list at `$5be4`, which `$14c42 movea.l (a0)+,a5` walks one entry per
/// **collapse step** until `$14c72 tst.l (a0)` finds its zero terminator --
/// **48 entries**. A step is not a frame: see [`crate::GameState::tick_frame`].
pub const COLLAPSE_FRAMES: u16 = 48;

/// Number of collapse slots the ST has in flight at once. `$a386 moveq #3,D6`
/// -- four `dbf` iterations over `$779e`/`$77ae`/`$77be`/`$77ce`, `$10` bytes
/// apart. Part 11h retraction, `discr-pu8`.
pub const COLLAPSE_SLOTS: usize = 4;

/// One of the four tile-collapse slots `disc-core` now models, ST `$779e`.
///
/// **Part 11h retraction, closed (`discr-pu8`): the ST has FOUR of these, not
/// one.** The claim loop -- `$a386 moveq #3,D6; $a388 lea $779e.w,A2; $a38c
/// tst.b (A2); $a38e bne.b $a3b2 (busy, try the next); $a390 st (A2) (claim);
/// ...init...; $a3b2 lea ($10,A2),A2; $a3b6 dbf D6w,$a38c` -- is confirmed
/// byte-for-byte against the disassembly (`sub_a354`, Ghidra project
/// `tmp/ghidra_proj`): four slots at `$779e`, `$77ae`, `$77be`, `$77ce`,
/// sixteen (`$10`) bytes apart, `moveq #3,D6` giving exactly four `dbf`
/// iterations. **The `$a38c` claim-scan question this bead asked is answered:
/// it scans all four for the first free one (`tst.b`/`bne` on each), claims
/// that one (`st`) and stops -- it does not queue behind a busy slot, and does
/// not merely test `$779e`.** If all four read busy, the `dbf` exhausts and
/// falls through unclaimed: a destruction that finds no free slot drops its
/// collapse animation silently, which [`damage`] models by doing nothing.
///
/// UNVERIFIED IN THIS PROJECT SNAPSHOT: the earlier note's citation of `$a4bc`
/// for the *advance* loop (the one that walks all four slots once per outer
/// iteration and `jsr`s the per-slot advance at `$14ba4`, called from `$96b6`)
/// could not be re-confirmed here -- `$a4bc` falls in a span with no Ghidra
/// function and zero xrefs to `$14ba4` in this batch-analysed copy, so either
/// that region needs auto-analysis re-run or the address was transcribed from
/// interactive analysis this snapshot does not carry. Not treated as a
/// retraction: the claim-side addresses above (`$a386`-`$a3b6`, containing the
/// previously-cited `$a38c`-`$a390`) match exactly, and [`collapse_step`]
/// below (the advance) is unchanged in its per-slot logic -- only the number
/// of slots it now runs over.
///
/// Modelling one slot was correct only while no trace destroyed two tiles
/// inside 50 collapse steps -- none of the three fixtures does, so landing
/// four slots is behavior-preserving on all three; see
/// `reports/part12-tiles.md` for what a two-collapse trace would need.
///
/// The life of one, read off `--watch 0x779e 0x77b0` over
/// `tests/fixtures/tile_damage.ndjson`:
///
/// ```text
/// tick  69  $a390  st $779e            claimed, busy = $ff
///           $a3ac  $77a2 = $7686       the target: the struck cell + 8
/// tick  70  $14c7a                     the frame cursor advances, 48 times,
///  ..  117                             one entry of $5be4 per tick
/// tick 117  $14c76 addq.b #2,(a6)      the list ran out: busy = $01, positive
/// tick 118  $14bb2 subq.b #3,(a6)      -> $fe
///           $14bb8 clr.w (a0)          THE WALKABILITY COPY IS CLEARED
///           $14c76 addq.b #2,(a6)      -> $00, the slot is free again
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Collapse {
    /// The ST's busy byte, `$779e` itself. `$ff` while the sprite list is still
    /// running, `$01` for the one frame after it ends, `0` when the slot frees.
    ///
    /// Modelling the byte rather than a frame count is what gets the timing
    /// right without a fudge: `$14bac tst.b (a6); bmi` sends a **negative** byte
    /// to the blitter, so the claiming tick's own pass advances the sprite
    /// cursor and does not count down anything else.
    pub busy: u8,
    /// The cell whose type word will be cleared: the struck cell plus
    /// [`WALK_COPY_OFFSET`]. ST `$77a2`.
    pub cell: usize,
    /// Animation frames still to run. ST: the distance from `$77aa` to the
    /// terminator of the `$5be4` list.
    pub frames_left: u16,
}

/// Advance every collapse slot by one frame, clearing the walkability copy
/// when a slot's animation is done. ST `$14ba4`-`$14bb8`, run for all four
/// slots by the advance loop `$96b6` calls once per outer iteration (`moveq
/// #3,D6; tst.b (A6); beq (skip a free slot); jsr $14ba4; lea ($10,A6),A6;
/// dbra D6`) -- same stride and slot order as the claim loop in [`damage`].
///
/// Called **before** the disc loop, so a collapse claimed by a destruction this
/// tick is not advanced until the next one -- which is what puts the clear 49
/// ticks after the destroy rather than 48.
pub fn collapse_step(
    slots: &mut [Option<Collapse>; COLLAPSE_SLOTS],
    tiles: &mut [Tile; TILE_CELLS],
    events: &mut Vec<Event>,
) {
    for slot in slots.iter_mut() {
        let Some(c) = slot else { continue };

        // $14bac tst.b (a6); bmi $14c20 -- still animating.
        if c.busy & 0x80 != 0 {
            if c.frames_left > 0 {
                // $14c42/$14c7a: one cell of the $5be4 list per pass.
                c.frames_left -= 1;
            } else {
                // $14c72 tst.l (a0); $14c76 addq.b #2,(a6) -- the list ran
                // out, and $ff + 2 is $01, which is positive.
                c.busy = c.busy.wrapping_add(2);
            }
            continue;
        }
        // $14bb2 subq.b #3, then $14bb8 clr.w (a0): the type only. The hp
        // word is left alone, which is why the fixture reads (1,1) -> (0,1)
        // and not (0,0), and $14c76's second addq takes the byte to 0 and
        // frees the slot.
        if let Some(t) = tiles.get_mut(c.cell) {
            t.tile_type = TILE_TYPE_DESTROYED;
            events.push(Event::TileDestroyed { cell: c.cell });
        }
        *slot = None;
    }
}

/// Apply `damage` to one cell, clamping HP at 0 and destroying the cell when
/// it reaches 0.
///
/// `damage` is the striking disc's `+$16` field, which `$a31c` subtracts —
/// it is a per-disc value, not a constant (every tier-1 hit observed in
/// `docs/disc-notes.md` took 3 only because that disc carried 3).
///
/// Pushes [`Event::TileDamaged`] for a surviving cell, or
/// [`Event::TileDestroyed`] for a killing hit. `$a360` also queues sample 3
/// (`move.b #$03,$6c5c`) on destruction; sound is out of scope for
/// `disc-core`, so that store is deliberately not modelled.
///
/// UNKNOWN: a second, unidentified writer sets and later clears bit 7 of the
/// HP word — `(1,5) -> (1,133)`, `(0,0) -> (0,128)`. `$a34c` stores a plain
/// value and cannot produce it, so it is not modelled here. See bd discr-dc0.
///
/// # Panics
///
/// If `cell >= TILE_CELLS`. The ST indexes `($02,a0,d5.w)` unchecked; the
/// caller is responsible for a valid cell index.
pub fn damage(
    tiles: &mut [Tile; TILE_CELLS],
    cell: usize,
    damage: i16,
    collapse: &mut [Option<Collapse>; COLLAPSE_SLOTS],
    events: &mut Vec<Event>,
) {
    let tile = &mut tiles[cell];

    // $a31c sub.w ($0016,a5),d6 / $a34a clr.w d6 -- clamped at 0, never negative.
    tile.hp = tile.hp.saturating_sub(damage).max(0);

    // $a34c move.w d6,($02,a0,d5.w) -- the HP store.
    if tile.hp == 0 {
        // $a354 clr.w ($00,a0,d5.w) -- HP == 0 also clears the TYPE word.
        tile.tile_type = TILE_TYPE_DESTROYED;
        events.push(Event::TileDestroyed { cell });
        // $a386-$a3b6: the destroy path scans all four slots in order
        // ($779e/$77ae/$77be/$77ce) for the first free one ($a38c tst.b (a2);
        // $a38e bne -- busy, try the next), claims it ($a390 st (a2)) and
        // points it at the struck cell plus eight. If the `dbf` exhausts all
        // four without finding one free, the ST falls through unclaimed --
        // the destroy's collapse animation is silently dropped, which is what
        // finding no `None` slot below models.
        if let Some(slot) = collapse.iter_mut().find(|s| s.is_none()) {
            *slot = Some(Collapse {
                // $a390 st (a2).
                busy: 0xff,
                cell: cell + WALK_COPY_OFFSET,
                frames_left: COLLAPSE_FRAMES,
            });
        }
    } else {
        events.push(Event::TileDamaged { cell, hp: tile.hp });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(tile_type: u16, hp: i16) -> [Tile; TILE_CELLS] {
        let mut tiles = [Tile::default(); TILE_CELLS];
        tiles[3] = Tile { tile_type, hp };
        tiles
    }

    /// One hit at cell 3, returning the cell after and the events emitted.
    fn hit(tile_type: u16, hp: i16, dmg: i16) -> (Tile, Vec<Event>) {
        let mut tiles = grid(tile_type, hp);
        let mut events = Vec::new();
        damage(
            &mut tiles,
            3,
            dmg,
            &mut [None, None, None, None],
            &mut events,
        );
        (tiles[3], events)
    }

    /// The clear lands after 48 collapse STEPS, however many frames those take.
    /// `p1_walk` destroys cell 6 at frame 188 and the ST clears cell 14 at 275,
    /// because only 85 of those frames ran an outer main-loop iteration.
    #[test]
    fn the_collapse_counts_steps_not_frames() {
        let mut tiles = grid(1, 1);
        let mut slots = [None, None, None, None];
        let mut ev = Vec::new();
        damage(&mut tiles, 3, 1, &mut slots, &mut ev);
        let cell = slots[0]
            .expect("the destroy claimed the first free slot")
            .cell;
        // 48 list entries, then one step for the terminator ($14c76 addq.b #2),
        // then the step that finds a positive byte and clears: 50.
        tiles[cell].tile_type = 2;
        for n in 0..49 {
            collapse_step(&mut slots, &mut tiles, &mut ev);
            assert_eq!(tiles[cell].tile_type, 2, "still animating after {n}");
            assert!(slots[0].is_some(), "slot still held after {n}");
        }
        collapse_step(&mut slots, &mut tiles, &mut ev);
        assert_eq!(tiles[cell].tile_type, TILE_TYPE_DESTROYED, "step 50 clears");
        assert!(slots[0].is_none(), "and frees the slot");
    }

    /// A destroy that finds all four slots busy drops its animation silently
    /// -- the ST's `dbf` exhausts without claiming ($a3b6). It still clears
    /// the struck cell's own type word ($a354), which does not depend on the
    /// collapse slot at all.
    #[test]
    fn a_fifth_destroy_with_all_slots_busy_drops_its_collapse() {
        let mut tiles = grid(1, 1);
        let mut slots = [
            Some(Collapse {
                busy: 0xff,
                cell: 0,
                frames_left: 1,
            }),
            Some(Collapse {
                busy: 0xff,
                cell: 1,
                frames_left: 1,
            }),
            Some(Collapse {
                busy: 0xff,
                cell: 2,
                frames_left: 1,
            }),
            Some(Collapse {
                busy: 0xff,
                cell: 4,
                frames_left: 1,
            }),
        ];
        let mut ev = Vec::new();
        damage(&mut tiles, 3, 1, &mut slots, &mut ev);
        assert_eq!(tiles[3].tile_type, TILE_TYPE_DESTROYED, "cell 3 still dies");
        assert_eq!(ev, vec![Event::TileDestroyed { cell: 3 }]);
        assert!(
            slots.iter().all(|s| s.is_some()),
            "no slot was claimed -- all four were already busy"
        );
    }

    /// The claim scans in slot order and takes the first free one, not
    /// necessarily slot 0 -- matching `$a386`'s `tst.b`/`bne` walk.
    #[test]
    fn the_claim_takes_the_first_free_slot_in_order() {
        let mut tiles = grid(1, 1);
        let mut slots = [
            Some(Collapse {
                busy: 0xff,
                cell: 0,
                frames_left: 1,
            }),
            None,
            Some(Collapse {
                busy: 0xff,
                cell: 2,
                frames_left: 1,
            }),
            None,
        ];
        let mut ev = Vec::new();
        damage(&mut tiles, 3, 1, &mut slots, &mut ev);
        assert!(slots[0].is_some(), "slot 0 untouched");
        assert_eq!(
            slots[1].expect("slot 1 was the first free one").cell,
            3 + WALK_COPY_OFFSET
        );
        assert!(slots[2].is_some(), "slot 2 untouched");
        assert!(slots[3].is_none(), "slot 3 never reached");
    }

    #[test]
    fn tier1_observed_transitions() {
        // docs/disc-notes.md, Part 8 tier 1: (2,4)->(2,1), (1,4)->(1,1),
        // (2,5)->(2,2), all with the damage 3 that disc carried.
        for (ty, hp, want_hp) in [(2u16, 4i16, 1i16), (1, 4, 1), (2, 5, 2)] {
            let (tile, events) = hit(ty, hp, 3);
            assert_eq!(
                tile,
                Tile {
                    tile_type: ty,
                    hp: want_hp
                }
            );
            assert_eq!(
                events,
                vec![Event::TileDamaged {
                    cell: 3,
                    hp: want_hp
                }]
            );
            assert!(tile.walkable());
        }
    }

    #[test]
    fn killing_hits_clear_the_type_word() {
        // $a354: (2,1)->(0,0) and (1,1)->(0,0) on the killing hit.
        for ty in [2u16, 1] {
            let (tile, events) = hit(ty, 1, 3);
            assert_eq!(
                tile,
                Tile {
                    tile_type: 0,
                    hp: 0
                }
            );
            assert_eq!(events, vec![Event::TileDestroyed { cell: 3 }]);
            assert!(!tile.walkable());
        }
    }

    #[test]
    fn damage_comes_from_the_disc_not_a_constant() {
        // $a31c subtracts disc+$16, so a disc carrying 1 takes 1.
        assert_eq!(
            hit(2, 4, 1).0,
            Tile {
                tile_type: 2,
                hp: 3
            }
        );
        assert_eq!(
            hit(2, 4, 2).0,
            Tile {
                tile_type: 2,
                hp: 2
            }
        );
    }

    #[test]
    fn hp_clamps_at_zero_never_negative() {
        // $a34a clr.w d6: an overkill hit lands on 0, not below it.
        let (tile, events) = hit(2, 1, 9);
        assert_eq!(
            tile,
            Tile {
                tile_type: 0,
                hp: 0
            }
        );
        assert_eq!(events, vec![Event::TileDestroyed { cell: 3 }]);
        assert_eq!(hit(2, 4, i16::MAX).0.hp, 0);
    }

    #[test]
    fn only_the_struck_cell_changes() {
        let mut tiles = [Tile {
            tile_type: 2,
            hp: 4,
        }; TILE_CELLS];
        let mut events = Vec::new();
        damage(&mut tiles, 0, 3, &mut [None, None, None, None], &mut events);
        assert_eq!(
            tiles[0],
            Tile {
                tile_type: 2,
                hp: 1
            }
        );
        assert!(tiles[1..].iter().all(|t| *t
            == Tile {
                tile_type: 2,
                hp: 4
            }));
    }
}
