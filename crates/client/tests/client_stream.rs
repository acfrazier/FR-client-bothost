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

#[test]
fn payload_byte_counters() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(&[1, 2, 3]).unwrap();
        let mut buf = [0u8; 4];
        sock.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, &[9, 8, 7, 6]);
    });
    let mut stream = ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap();
    assert_eq!(stream.bytes_in(), 0);
    assert_eq!(stream.bytes_out(), 0);
    // Read M = 3 payload bytes: 1 via read(), 2 via read_bytes().
    assert_eq!(stream.read().unwrap(), 1);
    let mut buf = [0u8; 2];
    stream.read_bytes(&mut buf, 0, 2).unwrap();
    assert_eq!(&buf, &[2, 3]);
    assert_eq!(stream.bytes_in(), 3);
    // Write N = 4 payload bytes.
    stream.write(&[9, 8, 7, 6], 4).unwrap();
    assert_eq!(stream.bytes_out(), 4);
    handle.join().unwrap();
    assert_eq!(stream.bytes_in(), 3);
    assert_eq!(stream.bytes_out(), 4);
}
