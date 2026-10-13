//! What "Examine" says. The text is the server's, not the client's: the game cache has none, and a
//! client only ever asks "what is object 1276?".
//!
//! `data/object_examines.json` holds texts from the OSRS Wiki for the objects placed in the squares
//! around Lumbridge and Draynor (by object id, with the name as a fallback for ids the wiki lists
//! under a sibling). An object it has nothing for gets a plain description from its name.

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Default, Deserialize)]
pub struct ExamineTexts {
    #[serde(default)]
    by_id: HashMap<u32, String>,
    /// Lower-case object name -> text.
    #[serde(default)]
    by_name: HashMap<String, String>,
}

impl ExamineTexts {
    /// The texts shipped with the game data.
    pub fn builtin() -> ExamineTexts {
        Self::from_json(include_str!("../../data/object_examines.json")).expect("object_examines.json is valid")
    }

    pub fn from_json(json: &str) -> Result<ExamineTexts, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The plain description of something called `name`, for when no text is listed.
    pub fn plain(name: &str) -> String {
        format!("It's {} {}.", crate::model::article(name), name.to_lowercase())
    }

    /// What examining object `id` (called `name`, when the server knows its name) says.
    pub fn object(&self, id: u32, name: Option<&str>) -> String {
        if let Some(text) = self.by_id.get(&id) {
            return text.clone();
        }
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return "You see nothing special.".to_owned();
        };
        if let Some(text) = self.by_name.get(&name.to_lowercase()) {
            return text.clone();
        }
        Self::plain(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts() -> ExamineTexts {
        ExamineTexts::from_json(
            r#"{"by_id": {"1276": "A commonly found tree."}, "by_name": {"bank booth": "The bank teller will serve you from here."}}"#,
        )
        .unwrap()
    }

    #[test]
    fn an_id_with_a_text_says_it() {
        assert_eq!(texts().object(1276, Some("Tree")), "A commonly found tree.");
    }

    #[test]
    fn an_id_without_one_falls_back_to_its_name_then_to_a_plain_description() {
        let texts = texts();
        assert_eq!(texts.object(1, Some("Bank booth")), "The bank teller will serve you from here.");
        assert_eq!(texts.object(2, Some("Gate")), "It's a gate.");
        assert_eq!(texts.object(3, Some("Oak tree")), "It's an oak tree.");
        assert_eq!(texts.object(4, None), "You see nothing special.");
    }

    #[test]
    fn the_shipped_texts_parse_and_know_the_bank_booth() {
        let texts = ExamineTexts::builtin();
        assert_eq!(texts.object(0, Some("Bank booth")), "The bank teller will serve you from here.");
    }
}
