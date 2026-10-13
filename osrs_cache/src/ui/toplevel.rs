//! The three top-level interfaces (fixed 548, resizable classic 161, resizable modern 164): where things
//! mount, and the load-time state the client scripts would produce.
//!
//! Component ids come from RuneLite's gameval `InterfaceID` (`Toplevel`, `ToplevelOsrsStretch`,
//! `ToplevelPreEoc`) and were checked against the cache dumps (sprites/positions agree).

use super::tree::{Override, UiState};
use crate::interface::{load_group, Component};
use crate::{Cache, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Fixed,
    Classic,
    Modern,
}

#[derive(Debug, Clone)]
pub struct TopLevel {
    pub group: u32,
    pub viewport: u32,
    pub chat_container: u32,
    pub orbs: u32,
    pub map_container: u32,
    pub minimap: u32,
    pub compass_click: u32,
    pub side_container: u32,
    pub side: [u32; 14],
    pub stone: [u32; 14],
    pub icon: [u32; 14],
}

/// Standard sub-interface group of each side tab.
pub const TAB_GROUPS: [u32; 14] = [593, 320, 629, 149, 387, 541, 218, 707, 109, 429, 182, 116, 216, 239];

fn ids(group: u32, v: impl IntoIterator<Item = u32>) -> [u32; 14] {
    let mut out = [0; 14];
    for (i, f) in v.into_iter().enumerate() {
        out[i] = group << 16 | f;
    }
    out
}

impl TopLevel {
    pub fn of(layout: Layout) -> TopLevel {
        match layout {
            Layout::Fixed => TopLevel {
                group: 548,
                viewport: 548 << 16 | 26,
                chat_container: 548 << 16 | 11,
                orbs: 548 << 16 | 25,
                map_container: 548 << 16 | 9,
                minimap: 548 << 16 | 22,
                compass_click: 548 << 16 | 24,
                side_container: 548 << 16 | 17,
                side: ids(548, 81..95),
                stone: ids(548, (64..71).chain(48..55)),
                icon: ids(548, (71..78).chain(55..62)),
            },
            Layout::Classic => TopLevel {
                group: 161,
                viewport: 161 << 16 | 91,
                chat_container: 161 << 16 | 96,
                orbs: 161 << 16 | 33,
                map_container: 161 << 16 | 95,
                minimap: 161 << 16 | 30,
                compass_click: 161 << 16 | 31,
                side_container: 161 << 16 | 73,
                side: ids(161, 76..90),
                stone: ids(161, (59..66).chain(43..50)),
                icon: ids(161, (66..73).chain(50..57)),
            },
            Layout::Modern => TopLevel {
                group: 164,
                viewport: 164 << 16 | 88,
                chat_container: 164 << 16 | 93,
                orbs: 164 << 16 | 33,
                map_container: 164 << 16 | 92,
                minimap: 164 << 16 | 30,
                compass_click: 164 << 16 | 31,
                side_container: 164 << 16 | 96,
                side: ids(164, 73..87),
                stone: ids(164, (52..59).chain([38, 39, 40, 34, 41, 42, 43])),
                icon: ids(164, (59..66).chain([44, 45, 46, 35, 47, 48, 49])),
            },
        }
    }
}

/// Load the top level, chat (162), orbs (160) and every standard side-tab group, mount them and emulate
/// the load-time scripts: only `selected_tab`'s SIDE pane is shown; on each STONE only the selected
/// tab's highlight sprite is drawn (the others get sprite -1: invisible but still clickable).
pub fn standard_state(cache: &Cache, layout: Layout, selected_tab: Option<usize>) -> Result<UiState> {
    let t = TopLevel::of(layout);
    let mut s = UiState { top: t.group, ..Default::default() };
    s.groups.insert(t.group, load_group(cache, t.group)?);
    for g in [162, 160] {
        s.groups.insert(g, load_group(cache, g)?);
    }
    s.mounts.insert(t.chat_container, 162);
    s.mounts.insert(t.orbs, 160);
    for (i, &g) in TAB_GROUPS.iter().enumerate() {
        let mut group = g;
        let mut comps = load_group(cache, group).unwrap_or_default();
        if comps.is_empty() && g == 629 {
            group = 399;
            comps = load_group(cache, group).unwrap_or_default();
        }
        if comps.is_empty() {
            continue;
        }
        s.groups.insert(group, comps);
        s.mounts.insert(t.side[i], group);
    }
    // The chat background: script 925 (chatbox onLoad) puts sprite 1017 (519x142) in the empty fill layer
    // 162:37, under the message text.
    s.created.entry(162 << 16 | 37).or_default().push(Component {
        is_if3: true,
        kind: 5,
        sprite_id: 1017,
        no_click_through: true,
        width_mode: 1,
        height_mode: 1,
        ..Default::default()
    });
    // Script 923 also draws a 1 px line (0x807660) 15 px above the bottom of the message box 162:57, over
    // the typing line.
    s.created.entry(162 << 16 | 57).or_default().push(Component {
        is_if3: true,
        kind: 3,
        filled: true,
        colour: 0x807660,
        x_mode: 1,
        y: 15,
        y_mode: 2,
        width_mode: 1,
        height: 1,
        ..Default::default()
    });
    // Script 920 (the end of each toplevel's resize) sets the minimap's mask: sprite 2154 on the resizable
    // layouts, 2422 on the fixed one (the cache widget still has the old 1178 / 1183).
    s.overrides.entry(t.minimap).or_default().sprite = Some(if layout == Layout::Fixed { 2422 } else { 2154 });
    // Script 2396 hides the Store orb (160:43) unless varbit 13037 is set (it is not by default).
    for file in 44..=47 {
        s.overrides.entry(160 << 16 | file).or_default().hidden = Some(true);
    }
    for i in 0..14 {
        let sel = selected_tab == Some(i);
        s.overrides.insert(t.side[i], Override { hidden: Some(!sel), ..Default::default() });
        if !sel {
            s.overrides.insert(t.stone[i], Override { sprite: Some(-1), ..Default::default() });
        }
    }
    if layout == Layout::Modern {
        // The right-hand stone row holds tabs 7, 9, 8, 11, 12, 13 (left to right, 33 px apart from the right
        // edge); the cache's static positions are in a scrambled order the script rewrites. Tab 10 (logout)
        // lives beside the minimap.
        for (k, tab) in [7usize, 9, 8, 11, 12, 13].into_iter().enumerate() {
            let x = 165 - 33 * k as i32;
            for id in [t.stone[tab], t.icon[tab]] {
                s.overrides.entry(id).or_default().pos = Some((x, 0));
            }
        }
    }
    Ok(s)
}

/// The width of the chat holder (164:93) script 920 subtracts from the client width.
const MODERN_CHAT_WIDTH: i32 = 519;
/// The room the one-row stone arrangement needs beside the chat holder (script 920: `width - chat < 429`
/// means two rows). 13 stones of 33 px.
const MODERN_ROW_WIDTH: i32 = 429;
/// The height of a stone row (164:94 / 164:95).
const MODERN_ROW_HEIGHT: i32 = 36;
/// The width of a stone row layer (164:94 / 164:95), and the 33 px of it that overlap in one row.
const MODERN_ROW_LAYER: i32 = 231;
const MODERN_STONE: i32 = 33;

/// Where script 920 puts the modern layout's stone rows and side panel, all anchored to the window's
/// bottom right corner (offsets are from the right and bottom edges).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModernArrangement {
    /// One row of 13 stones (else two rows).
    pub wide: bool,
    /// 164:95, the layer of tabs 0..=6.
    pub upper_stones: (i32, i32),
    /// 164:96, the side panel.
    pub panel: (i32, i32),
}

/// Script 920 for toplevel 1131 (modern): `client width - chat holder width < 429` stacks the two stone
/// layers (tabs 0..=6 above the rest, the panel above both), otherwise they stand side by side with the
/// panel above the row. `client_width` is the whole interface's width in game pixels.
pub fn modern_arrangement(client_width: i32) -> ModernArrangement {
    let row = MODERN_ROW_HEIGHT;
    if client_width - MODERN_CHAT_WIDTH < MODERN_ROW_WIDTH {
        ModernArrangement { wide: false, upper_stones: (0, row), panel: (0, row + row) }
    } else {
        ModernArrangement { wide: true, upper_stones: (MODERN_ROW_LAYER - MODERN_STONE, 0), panel: (0, row) }
    }
}

/// The parts of the toplevel scripts (907/920) that depend on the interface's size, applied to `state` (the
/// picture is `width` game pixels wide). Cheap: run it before every draw.
pub fn arrange(state: &mut UiState, layout: Layout, width: i32) {
    if layout == Layout::Modern {
        let a = modern_arrangement(width);
        state.overrides.entry(164 << 16 | 95).or_default().pos = Some(a.upper_stones);
        state.overrides.entry(164 << 16 | 96).or_default().pos = Some(a.panel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_stones_are_one_row_from_948_pixels() {
        // wide: the 13 stones side by side (the second layer 198 px left of the first), panel above the row
        let wide = modern_arrangement(948);
        assert!(wide.wide);
        assert_eq!(wide.upper_stones, (198, 0));
        assert_eq!(wide.panel, (0, 36));
        assert_eq!(modern_arrangement(1920), wide);
        // narrow: two rows of 6 and 7 stones, the panel above both
        let narrow = modern_arrangement(947);
        assert!(!narrow.wide);
        assert_eq!(narrow.upper_stones, (0, 36));
        assert_eq!(narrow.panel, (0, 72));
        assert_eq!(modern_arrangement(765), narrow);
    }

    #[test]
    fn arrange_only_touches_the_modern_layout() {
        let mut s = UiState::default();
        arrange(&mut s, Layout::Classic, 500);
        assert!(s.overrides.is_empty());
        arrange(&mut s, Layout::Modern, 500);
        assert_eq!(s.overrides[&(164 << 16 | 95)].pos, Some((0, 36)));
        assert_eq!(s.overrides[&(164 << 16 | 96)].pos, Some((0, 72)));
        arrange(&mut s, Layout::Modern, 1200);
        assert_eq!(s.overrides[&(164 << 16 | 95)].pos, Some((198, 0)));
    }
}
