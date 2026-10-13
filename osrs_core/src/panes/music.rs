//! The Music Player tab (group 239; scripts 250, 9290, 9300, 318, 31).
//!
//! The track list is database table 44 (one row per track: column 0 the name it sorts by,
//! column 1 the name shown, column 2 where it unlocks). Script 9290 creates one text line per
//! track in the layer 239:11: font 495, left aligned, `y = 3 + n * 15`, in the colour of its state
//! (the game's red for a locked track, green once unlocked, white while playing). The simulation
//! has no music, so every track is drawn locked. The scroll bar is script 31's (239:12).
//! Only the lines that fit in the 154 px high list are created; the pane never scrolls.
//!
//! The four mode buttons (Area, Shuffle, Single, Skip: script 318) are the 23x18 sprites 7427,
//! 7428, 7429 and 7430 centred in the buttons 239:14..17. The "Playing:" line is static; the name
//! of the track (239:4) and the footer (239:5) are left empty.

use super::{comp, scrollbar, PaneInput};
use osrs_cache::dbtable::DbTables;
use osrs_cache::interface::Component;
use osrs_cache::ui::UiState;
use osrs_cache::Cache;

/// The table of music tracks.
const TRACKS: u32 = 44;
/// A locked track's colour in the list.
const LOCKED: u32 = 0xdc1010;
/// Rows are 15 px high; the list is 154 px high.
const ROW: i32 = 15;
const VIEW: i32 = 154;

/// Every track's displayed name, in the list's order (sorted by the sort name).
#[derive(Default)]
pub struct MusicTable {
    pub names: Vec<String>,
}

impl MusicTable {
    pub fn load(cache: &Cache) -> osrs_cache::Result<MusicTable> {
        let tables = DbTables::load(cache)?;
        let mut rows: Vec<(String, String)> = tables
            .rows_of(TRACKS)
            .filter_map(|row| {
                let sort = row.tuple(0, 0)?.first()?.str()?.to_lowercase();
                let shown = row.tuple(1, 0)?.first()?.str()?.to_owned();
                (!shown.trim().is_empty()).then_some((sort, shown))
            })
            .collect();
        rows.sort();
        Ok(MusicTable { names: rows.into_iter().map(|(_, n)| n).collect() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_track_has_a_name_and_the_list_is_in_the_games_order() {
        let Ok(cache) = Cache::open_default() else { return };
        let table = MusicTable::load(&cache).unwrap();
        assert!(table.names.len() > 800);
        assert!(table.names.iter().all(|n| !n.trim().is_empty()));
        assert_eq!(table.names[0], "7th Realm");
    }
}

pub fn sync(state: &mut UiState, input: &PaneInput) {
    let names = &input.icons.music.names;
    if names.is_empty() {
        return;
    }
    let shown = (VIEW / ROW + 2) as usize;
    let lines = names
        .iter()
        .take(shown)
        .enumerate()
        .map(|(n, name)| Component {
            is_if3: true,
            kind: 4,
            x: 0,
            y: (3 + n as i32 * ROW) as i16,
            width: 0,
            height: ROW,
            width_mode: 1,
            x_mode: 1,
            font_id: 495,
            colour: LOCKED,
            text: name.clone(),
            x_align: 0,
            y_align: 1,
            shadowed: true,
            ..Default::default()
        })
        .collect();
    state.created.insert(comp(239, 11), lines);
    state.created.insert(comp(239, 12), scrollbar(VIEW, VIEW, 3 + names.len() as i32 * ROW));

    // the mode buttons: Area, Shuffle, Single, Skip
    for (file, sprite) in [(14, 7427), (15, 7428), (16, 7429), (17, 7430)] {
        state.created.insert(
            comp(239, file),
            vec![Component {
                is_if3: true,
                kind: 5,
                width: 23,
                height: 18,
                x_mode: 1,
                y_mode: 1,
                sprite_id: sprite,
                ..Default::default()
            }],
        );
    }
}
