//! Skill table, 1:1 with `webclient/src/client/Skill.ts`.

pub struct Skill;

// Names stay TS (`Skill.count` / `Skill.used`) for the 1:1 port.
#[allow(non_upper_case_globals)]
impl Skill {
    pub const count: usize = 25;
    pub const names: [&str; 25] = [
        "attack", "defence", "strength", "hitpoints", "ranged", "prayer", "magic", "cooking",
        "woodcutting", "fletching", "fishing", "firemaking", "crafting", "smithing", "mining",
        "herblore", "agility", "thieving", "slayer", "-unused-", "runecraft", "-unused-",
        "-unused-", "-unused-", "-unused-",
    ];
    pub const used: [bool; 25] = [
        true, true, true, true, true, true, true, true, true, true, true, true, true, true, true,
        true, true, true, false, false, true, false, false, false, false,
    ];
}
