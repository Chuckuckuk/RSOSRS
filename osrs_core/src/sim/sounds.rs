//! Sound effects the game plays for the player itself, whatever it is fighting: cache sound ids
//! (archive 4), named as Jagex names them (the OSRS Wiki's "List of sound IDs").
//! Monsters, weapons, spells, prayers and consumables name their own sounds in the data.

/// `human_hit`: a hit with damage lands on the player. The player is the male character, so this
/// is the human set, not 510 (`female_hit_2`, which RuneLite names `TAKE_DAMAGE_SPLAT`).
pub const PLAYER_HIT: u32 = 513;
/// `human_block_1` (RuneLite's `ZERO_DAMAGE_SPLAT`): an attack on the player deals 0.
pub const PLAYER_BLOCK: u32 = 511;
/// `human_death`
pub const PLAYER_DEATH: u32 = 512;
/// `unarmed_punch`: the player attacks with bare fists.
pub const UNARMED_ATTACK: u32 = 2566;
/// `spellfail`: a spell splashes.
pub const SPELL_SPLASH: u32 = 227;
/// `cancel_prayer`: the player switches a prayer off.
pub const PRAYER_OFF: u32 = 2663;
/// `prayer_drain`: prayer points run out and every prayer goes off.
pub const PRAYER_DRAINED: u32 = 2672;
/// `eat`: food without a sound of its own.
pub const EAT: u32 = 2393;
/// `liquid`: a potion sip. (`drink`, 2390, is a different, shorter sound; the Jad simulator's
/// potion recording is 1.42 s, which is exactly `liquid`.)
pub const DRINK: u32 = 2401;

/// Every sound id above.
pub const ALL: [u32; 9] = [PLAYER_HIT, PLAYER_BLOCK, PLAYER_DEATH, UNARMED_ATTACK, SPELL_SPLASH, PRAYER_OFF, PRAYER_DRAINED, EAT, DRINK];
