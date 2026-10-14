//! Firemaking in data form: which logs burn, at what level, for how much experience and for how
//! long the fire they make lasts. `sim::firemaking` runs it.

use serde::{Deserialize, Serialize};

/// One kind of log that can be lit with a tinderbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogDef {
    /// The log's item id (`"oak_logs"`).
    pub item: String,
    /// The Firemaking level needed to light it (boosts count).
    pub level: i32,
    /// Experience for lighting it, in tenths of a point.
    pub xp_tenths: i32,
    /// How many ticks the fire it makes burns before going out.
    pub burn_ticks: i32,
    /// How many ticks adding this log gives a bonfire (Forester's campfire). A bonfire made from
    /// it starts with this on top of `BONFIRE_BASE_TICKS`.
    pub bonfire_ticks: i32,
}
