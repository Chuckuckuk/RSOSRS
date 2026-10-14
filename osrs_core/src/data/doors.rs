//! Doors and gates in data form: which closed object turns into which open one, and how the
//! leaves swing. `sim::objects` opens and closes them.

use serde::{Deserialize, Serialize};

/// One leaf of a door: the object while it is shut, and the one while it stands open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorLeafDef {
    pub closed: u32,
    pub open: u32,
}

/// How a leaf moves when it opens, in the frame of a door on the west edge of its tile (the wall
/// running north): the tile it moves to and the quarter turns clockwise it makes. A leaf placed
/// turned `o` moves the same way turned `o` quarter turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeafMove {
    pub offset: (i32, i32),
    pub turn: i32,
}

/// How a door's leaves swing open. Leaves are taken in order along the wall: in the west-edge
/// frame, the first is the southern one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoorSwing {
    /// One leaf, hinged at its northern end: it swings out across the edge, a quarter turn
    /// clockwise.
    Single,
    /// Two leaves hinged at their outer ends, swinging out away from each other.
    Double,
    /// A two-leaf gate. Each leaf swings like a double door's, but only the one chosen moves.
    Gate,
}

impl DoorSwing {
    pub fn leaf_count(self) -> usize {
        self.moves().len()
    }

    /// Whether opening or closing one leaf moves them all. A gate's leaves move one at a time.
    pub fn leaves_move_together(self) -> bool {
        self != DoorSwing::Gate
    }

    /// How each leaf moves as it opens, in order along the wall.
    pub fn moves(self) -> &'static [LeafMove] {
        const OUT_CLOCKWISE: LeafMove = LeafMove { offset: (-1, 0), turn: 1 };
        const OUT_ANTICLOCKWISE: LeafMove = LeafMove { offset: (-1, 0), turn: -1 };
        match self {
            DoorSwing::Single => &[OUT_CLOCKWISE],
            DoorSwing::Double | DoorSwing::Gate => &[OUT_ANTICLOCKWISE, OUT_CLOCKWISE],
        }
    }
}

fn yes() -> bool {
    true
}

/// A kind of door or gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorDef {
    pub id: String,
    pub swing: DoorSwing,
    /// One leaf for a single door, two for a pair (in either order: which is which along the
    /// wall is worked out from where they stand).
    pub leaves: Vec<DoorLeafDef>,
    /// False for doors that stay open (bank doors).
    #[serde(default = "yes")]
    pub can_close: bool,
}

impl DoorDef {
    /// Whether `object` is one of this door's leaves, and whether that is its open form.
    pub fn state_of(&self, object: u32) -> Option<bool> {
        self.leaf(object).map(|leaf| leaf.open == object)
    }

    /// The leaf that `object` is a form of.
    pub fn leaf(&self, object: u32) -> Option<DoorLeafDef> {
        self.leaves.iter().copied().find(|leaf| leaf.closed == object || leaf.open == object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_door_knows_its_leaves_and_their_state() {
        let door = DoorDef {
            id: "d".into(),
            swing: DoorSwing::Double,
            leaves: vec![DoorLeafDef { closed: 1, open: 2 }, DoorLeafDef { closed: 3, open: 4 }],
            can_close: true,
        };
        assert_eq!(door.state_of(1), Some(false));
        assert_eq!(door.state_of(4), Some(true));
        assert_eq!(door.state_of(5), None);
        assert_eq!(door.leaf(4), Some(DoorLeafDef { closed: 3, open: 4 }));
    }
}
