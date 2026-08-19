use client::io::ClientStream;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

#[test]
fn tcp_read_write_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(&[7, 8, 9]).unwrap();
        let mut buf = [0u8; 2];
        sock.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, &[1, 2]);
    });
    let mut stream = ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap();
    assert_eq!(stream.read().unwrap(), 7);
    let mut buf = [0u8; 2];
    stream.read_bytes(&mut buf, 0, 2).unwrap();
    assert_eq!(&buf, &[8, 9]);
    stream.write(&[1, 2], 2).unwrap();
    handle.join().unwrap();
}
