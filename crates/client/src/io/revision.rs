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

/// Named 289 inbound opcodes that are source-verified for stage-1 use.
/// Other 289 IDs are framed via [`SERVER_PROT_SIZES_289`] but not dispatched
/// until later stages trace their fields.
pub struct ServerProt289;

impl ServerProt289 {
    pub const UPDATE_INV_FULL: i32 = 107;
    pub const UPDATE_INV_PARTIAL: i32 = 76;
    pub const LOGOUT: i32 = 121;
    pub const PLAYER_INFO: i32 = 188;
    pub const REBUILD_NORMAL: i32 = 219;
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
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::UPDATE_INV_FULL as usize],
            -2
        );
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::UPDATE_INV_PARTIAL as usize],
            -2
        );
        assert_eq!(SERVER_PROT_SIZES_289[ServerProt289::LOGOUT as usize], 0);
        assert_eq!(SERVER_PROT_SIZES_289[47], -2);
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::PLAYER_INFO as usize],
            -2
        );
        assert_eq!(
            SERVER_PROT_SIZES_289[ServerProt289::REBUILD_NORMAL as usize],
            4
        );
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
