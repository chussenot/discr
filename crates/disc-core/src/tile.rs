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
/// frame until `$14c72 tst.l (a0)` finds its zero terminator -- **48 entries**.
pub const COLLAPSE_FRAMES: u16 = 48;

/// The one tile collapse the ST can have in flight. ST `$779e`.
///
/// A **single slot**, claimed at `$a38c`-`$a390` with `tst.b (a2); bne` --
/// so a second tile destroyed while one is already collapsing gets no
/// animation, and *its* walkability copy is never cleared. That is a real
/// quirk, not a simplification: nothing in the code queues a second one.
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

/// Advance the collapse slot by one frame, clearing the walkability copy when
/// the animation is done. ST `$14ba4`-`$14bb8`.
///
/// Called **before** the disc loop, so a collapse claimed by a destruction this
/// tick is not advanced until the next one -- which is what puts the clear 49
/// ticks after the destroy rather than 48.
pub fn collapse_step(
    slot: &mut Option<Collapse>,
    tiles: &mut [Tile; TILE_CELLS],
    events: &mut Vec<Event>,
) {
    let Some(c) = slot else { return };

    // $14bac tst.b (a6); bmi $14c20 -- still animating.
    if c.busy & 0x80 != 0 {
        if c.frames_left > 0 {
            // $14c42/$14c7a: one cell of the $5be4 list per pass.
            c.frames_left -= 1;
        } else {
            // $14c72 tst.l (a0); $14c76 addq.b #2,(a6) -- the list ran out, and
            // $ff + 2 is $01, which is positive.
            c.busy = c.busy.wrapping_add(2);
        }
        return;
    }
    // $14bb2 subq.b #3, then $14bb8 clr.w (a0): the type only. The hp word is
    // left alone, which is why the fixture reads (1,1) -> (0,1) and not (0,0),
    // and $14c76's second addq takes the byte to 0 and frees the slot.
    if let Some(t) = tiles.get_mut(c.cell) {
        t.tile_type = TILE_TYPE_DESTROYED;
        events.push(Event::TileDestroyed { cell: c.cell });
    }
    *slot = None;
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
    collapse: &mut Option<Collapse>,
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
        // $a388-$a3ac: the destroy path claims the single collapse slot, unless
        // one is already running ($a38c tst.b (a2); bne), and points it at the
        // struck cell plus eight.
        if collapse.is_none() {
            *collapse = Some(Collapse {
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
        damage(&mut tiles, 3, dmg, &mut None, &mut events);
        (tiles[3], events)
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
        damage(&mut tiles, 0, 3, &mut None, &mut events);
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
