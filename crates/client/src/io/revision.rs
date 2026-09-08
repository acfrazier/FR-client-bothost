//! Explicit client/session protocol revision selection.
//!
//! Default construction stays on revision 274 with the existing public
//! `ServerProt` / `ClientProt` / `SERVER_PROT_SIZES` tables unchanged.
//! Revision 289 is additive: source-derived inbound sizes and named
//! opcodes live beside the 274 tables and are selected only when a
//! session opts in.

use super::server_prot::SERVER_PROT_SIZES;

/// Protocol revision bound at client/session construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientRevision {
    /// Existing 274 bothost/client-play path (default).
    #[default]
    R274,
    /// Source-grounded 289 offline/live profile (explicit opt-in).
    R289,
}

impl ClientRevision {
    pub const fn as_i32(self) -> i32 {
        match self {
            ClientRevision::R274 => 274,
            ClientRevision::R289 => 289,
        }
    }

    /// Inbound frame-size table for this revision (`ptype` is a byte index).
    pub fn server_prot_sizes(self) -> &'static [i32; 256] {
        match self {
            ClientRevision::R274 => &SERVER_PROT_SIZES,
            ClientRevision::R289 => &SERVER_PROT_SIZES_289,
        }
    }

    pub fn is_274(self) -> bool {
        matches!(self, ClientRevision::R274)
    }

    pub fn is_289(self) -> bool {
        matches!(self, ClientRevision::R289)
    }
}

/// Named 289 inbound opcodes source-verified for stage-1/2 dispatch.
/// Other 289 IDs are framed via [`SERVER_PROT_SIZES_289`] but fail-closed
/// until a later stage traces their fields.
pub struct ServerProt289;

impl ServerProt289 {
    // Stage 1
    pub const UPDATE_INV_FULL: i32 = 107;
    pub const UPDATE_INV_PARTIAL: i32 = 76;
    // Startup chat modes (primary client.java:2612-2619).
    pub const CHAT_FILTER_SETTINGS: i32 = 13;
    pub const IF_CLOSE: i32 = 23;
    pub const UPDATE_IGNORELIST: i32 = 47;
    pub const FRIENDLIST_LOADED: i32 = 235;
    pub const UPDATE_PID: i32 = 120;
    // Stage 2 — login/actors/world/widgets/reset (client.java dispatch)
    pub const LOGOUT: i32 = 121;
    pub const PLAYER_INFO: i32 = 188;
    pub const NPC_INFO: i32 = 65;
    pub const REBUILD_NORMAL: i32 = 219;
    pub const RESET_ANIMS: i32 = 201;
    pub const IF_SETTEXT: i32 = 59;
    pub const IF_SETANIM: i32 = 211;
    pub const IF_OPENMAIN_SIDE: i32 = 55;
    pub const IF_OPENSIDE: i32 = 252;
    pub const IF_OPENOVERLAY: i32 = 127;
    pub const VARP_SMALL: i32 = 75;
    pub const VARP_LARGE: i32 = 97;
    pub const VARP_SYNC: i32 = 172;
}

// 289 `Class17.anIntArray209` lengths, generated from
// `docs/revision-289/protocol-289.json` (primary pin 0c00ef24).
include!("server_prot_sizes_289.inc.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_289_named_rows() {
        assert_eq!(SERVER_PROT_SIZES_289.len(), 256);
        // Stage 1
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::UPDATE_INV_FULL as usize],
            -2
        );
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::UPDATE_INV_PARTIAL as usize],
            -2
        );
        assert_eq!(SERVER_PROT_SIZES_289[47], -2);
        // Stage 2 named opcodes (protocol-289.json / Class17 lengths)
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::PLAYER_INFO as usize],
            -2
        );
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::NPC_INFO as usize], -2);
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::REBUILD_NORMAL as usize],
            4
        );
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::LOGOUT as usize], 0);
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::RESET_ANIMS as usize],
            0
        );
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::IF_SETTEXT as usize],
            -2
        );
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::IF_SETANIM as usize], 4);
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::IF_OPENMAIN_SIDE as usize],
            4
        );
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::IF_OPENSIDE as usize],
            2
        );
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::IF_OPENOVERLAY as usize],
            2
        );
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::VARP_SMALL as usize], 3);
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::VARP_LARGE as usize], 6);
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::VARP_SYNC as usize], 0);
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::CHAT_FILTER_SETTINGS as usize],
            3
        );
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::IF_CLOSE as usize], 0);
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::UPDATE_IGNORELIST as usize], -2);
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::FRIENDLIST_LOADED as usize], 1);
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::UPDATE_PID as usize], 3);
    }

    #[test]
    fn default_revision_is_274_and_keeps_public_table() {
        assert_eq!(ClientRevision::default(), ClientRevision::R274);
        assert_eq!(
            ClientRevision::R274.server_prot_sizes() as &[i32],
            &SERVER_PROT_SIZES as &[i32]
        );
        assert_ne!(
            ClientRevision::R289.server_prot_sizes() as &[i32],
            &SERVER_PROT_SIZES as &[i32]
        );
        // 274 public logout size remains on the default profile.
        assert_eq!(ClientRevision::R274.server_prot_sizes()[88], 0);
        assert_eq!(ClientRevision::R289.server_prot_sizes()[121], 0);
    }
}
