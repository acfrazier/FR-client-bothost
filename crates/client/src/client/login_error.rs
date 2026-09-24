//! Login handshake error, mirroring the response code and the two
//! `loginMes` lines the 274 client would show on the title screen.

#[derive(Debug)]
pub struct LoginError {
    pub code: i32,
    pub mes1: String,
    pub mes2: String,
    /// Server-directed delay before retrying the same endpoint. Present for
    /// response 21 (world-hop profile transfer cooldown).
    pub retry_after: Option<std::time::Duration>,
}
