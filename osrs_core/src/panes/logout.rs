//! The Logout tab (group 182; scripts 2154, 2243, 2244, 970).
//!
//! Script 2243 (run on load) lays the pane out for the desktop client with the feedback survey
//! turned off (varbit 6272 is 0): the layer 182:1 that holds the text and both buttons is 170 px
//! high and sits 24 px above the bottom. The text on top is static ("Use the buttons below to
//! logout or switch worlds safely."), the World Switcher button is centred and the Logout button
//! sits at the bottom; script 2244 writes "Click here to logout" into the Logout button (the
//! mobile client says "Tap here"). The buttons' words were drawn by content-typed text
//! components, which the renderer now draws like any other text.
//!
//! Clicking Logout runs script 970 (the `logout` instruction); this game has no log-in screen, so
//! the client answers with a chat message instead (`UiTarget::Logout`).

use super::{comp, PaneInput};
use osrs_cache::ui::{Override, UiState};

pub fn sync(state: &mut UiState, _input: &PaneInput) {
    state.overrides.insert(comp(182, 1), Override { pos: Some((0, 24)), size: Some((0, 170)), ..Default::default() });
    state.overrides.insert(comp(182, 12), Override { text: Some("Click here to logout".to_owned()), ..Default::default() });
}
