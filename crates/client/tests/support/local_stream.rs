//! A local socket for protocol fixtures, not a production connection path.
//! Hosted builds do not expose a native dial; their protocol tests still
//! need a connected stream independent of the process-wide host network.

use client::io::ClientStream;
use std::io;

pub fn connect(host: &str, port: u16) -> io::Result<ClientStream> {
    #[cfg(feature = "standalone-native-transport")]
    {
        ClientStream::connect(host, port)
    }
    #[cfg(not(feature = "standalone-native-transport"))]
    {
        ClientStream::connect_tcp_for_test(host, port)
    }
}
