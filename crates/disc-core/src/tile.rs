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
//! [`Tile::walkable`] is that predicate. Nothing here reads or writes a cell
//! per frame: on the ST, cells change only when a disc hits one, so
//! [`crate::disc::step`] calls [`damage`] from inside the disc loop.

use crate::{Event, TILE_CELLS, TILE_TYPE_DESTROYED, Tile};

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
pub fn damage(tiles: &mut [Tile; TILE_CELLS], cell: usize, damage: i16, events: &mut Vec<Event>) {
    let tile = &mut tiles[cell];

    // $a31c sub.w ($0016,a5),d6 / $a34a clr.w d6 -- clamped at 0, never negative.
    tile.hp = tile.hp.saturating_sub(damage).max(0);

    // $a34c move.w d6,($02,a0,d5.w) -- the HP store.
    if tile.hp == 0 {
        // $a354 clr.w ($00,a0,d5.w) -- HP == 0 also clears the TYPE word.
        tile.tile_type = TILE_TYPE_DESTROYED;
        events.push(Event::TileDestroyed { cell });
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
        damage(&mut tiles, 3, dmg, &mut events);
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
        damage(&mut tiles, 0, 3, &mut events);
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
