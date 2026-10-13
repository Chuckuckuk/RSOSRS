//! The chatbox's message lines (group 162; scripts 923 and the chat redraws).
//!
//! The box (162:59, inside the separator layer 162:57) holds one text widget per message line,
//! 162:60 onward, font 495 (p12_full), black on the parchment for game messages. The newest line
//! sits at the bottom, the older ones above it; a message too wide for the box is wrapped
//! first. 162:58, the line under them, is the typing line: "Name: *" with the cursor in blue.
//!
//! TODO: public/private/clan chat colours, scrolling back through older lines and the scrollbar.

use super::{comp, PaneInput};
use osrs_cache::font::Font;
use osrs_cache::ui::text::text_width;
use osrs_cache::ui::{Assets, Override, UiState};

/// Height of one line, pixels.
pub const LINE: i32 = 14;
/// Height of the scrolling box (162:59) at the chatbox's size: the 165 high box less the 23 of
/// buttons and the 12 and 16 of margin and typing line.
const BOX_HEIGHT: i32 = 114;
/// Width a line may fill before it wraps.
const WRAP_WIDTH: i32 = 480;
/// Component of the first line.
const FIRST_LINE: u32 = 60;

/// Splits `text` into lines no wider than `max` pixels, at spaces.
pub fn wrap(font: &Font, text: &str, max: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        let candidate = if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
        if current.is_empty() || text_width(font, &candidate) <= max {
            current = candidate;
        } else {
            lines.push(std::mem::replace(&mut current, word.to_owned()));
        }
    }
    lines.push(current);
    lines
}

pub fn sync(state: &mut UiState, input: &PaneInput, assets: &mut dyn Assets) {
    let Some(font) = assets.font(495) else { return };
    let mut lines: Vec<String> = Vec::new();
    for message in input.chat.iter().rev().take(12) {
        // a message wraps to several lines; keep them in reading order
        let mut wrapped = wrap(font, message, WRAP_WIDTH);
        wrapped.reverse();
        lines.extend(wrapped);
        if lines.len() >= 12 {
            break;
        }
    }
    // `lines` is newest first; show the newest ones that fit, oldest of them first
    let fit = (BOX_HEIGHT / LINE) as usize;
    lines.truncate(fit);
    lines.reverse();
    let n = lines.len() as i32;
    for (k, line) in lines.into_iter().enumerate() {
        let y = BOX_HEIGHT - LINE * (n - k as i32);
        state.overrides.insert(
            comp(162, FIRST_LINE + k as u32),
            Override { text: Some(line), pos: Some((3, y + 1)), ..Default::default() },
        );
    }
    // clear the lines a longer history used before
    for k in n as u32..fit as u32 {
        state.overrides.remove(&comp(162, FIRST_LINE + k));
    }
    state.overrides.insert(
        comp(162, 58),
        Override { text: Some(format!("{}: <col=0000ff>*</col>", input.name)), ..Default::default() },
    );
}
