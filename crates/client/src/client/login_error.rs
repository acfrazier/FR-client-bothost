//! Login handshake error, mirroring the response code and the two
//! `loginMes` lines the 274 client would show on the title screen.

pub struct LoginError {
    pub code: i32,
    pub mes1: String,
    pub mes2: String,
}
