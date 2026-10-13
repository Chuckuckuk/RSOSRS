//! The tab stones and the side panel's open/closed state (scripts 916 and 2646, the end of 907).
//!
//! Only the open tab's pane is shown and only its stone is drawn red; every other stone gets sprite
//! -1 (drawn as nothing, but still clickable). With the panel closed (resizable modern only) no pane
//! is shown, the panel's container is hidden and no stone is red.

use super::{comp, PaneInput};
use osrs_cache::ui::{Layout, Override, UiState};

/// Static widgets of the classic panel that go with it: the stone backdrop and the two pillars.
fn backdrop(layout: Layout) -> Vec<u32> {
    match layout {
        Layout::Classic => vec![comp(161, 38), comp(161, 39), comp(161, 40)],
        _ => Vec::new(),
    }
}

pub fn sync(state: &mut UiState, input: &PaneInput) {
    let t = input.top;
    for i in 0..14 {
        let shown = input.selected == i && input.open;
        state.overrides.entry(t.side[i]).or_insert_with(Override::default).hidden = Some(!shown);
        state.overrides.entry(t.stone[i]).or_insert_with(Override::default).sprite = if shown { None } else { Some(-1) };
    }
    let hide = !input.open;
    if t.side_container != 0 {
        state.overrides.entry(t.side_container).or_default().hidden = Some(hide);
    }
    for id in backdrop(input.layout) {
        state.overrides.entry(id).or_default().hidden = Some(hide);
    }
}
