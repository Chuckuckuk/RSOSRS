//! Game messages from the server (examine texts, rejected commands, the welcome line...). They are
//! kept here and drawn by the interface inside the chatbox (`panes::chat`), the way the game
//! keeps them: the newest lines stay in the box until more push them out.

use bevy::prelude::*;
use std::collections::VecDeque;

/// Messages remembered at most.
const KEPT: usize = 100;

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatLog>();
    }
}

/// The messages received, newest last, with the time each arrived.
#[derive(Resource, Default)]
pub struct ChatLog {
    lines: VecDeque<(String, f32)>,
    /// Counts every message ever pushed, so the interface can tell something changed.
    pub total: u64,
}

impl ChatLog {
    pub fn push(&mut self, text: String, now: f32) {
        self.lines.push_back((text, now));
        self.total += 1;
        while self.lines.len() > KEPT {
            self.lines.pop_front();
        }
    }

    /// The remembered messages, oldest first.
    pub fn messages(&self) -> Vec<String> {
        self.lines.iter().map(|(t, _)| t.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_messages_are_kept() {
        let mut chat = ChatLog::default();
        for i in 0..KEPT + 10 {
            chat.push(format!("line {i}"), i as f32);
        }
        let all = chat.messages();
        assert_eq!(all.len(), KEPT);
        assert_eq!(all.last().unwrap(), &format!("line {}", KEPT + 9));
        assert_eq!(chat.total, (KEPT + 10) as u64);
    }
}
