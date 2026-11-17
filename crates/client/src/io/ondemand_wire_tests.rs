use super::*;
use std::io::Write;
use std::net::{TcpListener, TcpStream};

fn fixture() -> (Worker, mpsc::Receiver<WorkerMessage>, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let stream = ClientStream::connect("127.0.0.1", port).unwrap();
    let (server, _) = listener.accept().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    server.set_nodelay(true).unwrap();
    let (_, commands) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    let worker = Worker {
        commands,
        versions: vec![vec![1; 300]; 4],
        crcs: vec![vec![0; 300]; 4],
        priorities: vec![vec![0; 300]; 4],
        top_priority: 0,
        queue: VecDeque::new(),
        missing: VecDeque::new(),
        pending: Vec::new(),
        prefetches: VecDeque::new(),
        message: String::new(),
        fail_count: 0,
        urgent_count: 0,
        request_count: 0,
        loaded_prefetch_files: 0,
        total_prefetch_files: 0,
        buf: [0; 500],
        part_offset: 0,
        part_available: 0,
        packet_cycle: 0,
        no_timeout_cycle: 0,
        active: false,
        socket_open_time: Instant::now() - SOCKET_OPEN_GATE,
        current: None,
        stream: Some(stream),
        host: "127.0.0.1".into(),
        port,
        transport: Some(Transport::Tcp),
        revision: Some(ClientRevision::R377),
        cache_dir: None,
        persist_dir: None,
        map_archive: None,
        wake_on_response: false,
        java_cycle: Duration::from_millis(20),
        counted_at: Instant::now(),
        cold_connect: None,
        recovering: false,
        reconnect_backoff: Duration::ZERO,
        saw_network_progress: false,
        subs: Arc::new(Mutex::new(HashMap::from([(1, tx)]))),
        running: Arc::new(AtomicBool::new(true)),
        ingame: Arc::new(AtomicBool::new(false)),
    };
    (worker, rx, server)
}

fn available(worker: &mut Worker, count: i32) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while worker.stream.as_mut().unwrap().available().unwrap() < count {
        assert!(Instant::now() < deadline, "update fragment did not arrive");
        thread::yield_now();
    }
}

#[test]
fn java_update_hello_and_each_request_priority_match_wire() {
    let (mut worker, _, mut server) = fixture();
    for (urgent, ingame, priority) in [(true, false, 2), (false, false, 1), (false, true, 0)] {
        worker.ingame.store(ingame, Ordering::Relaxed);
        worker.no_timeout_cycle = 37;
        worker.send(3, 0x1234, urgent);
        let mut request = [0; 4];
        server.read_exact(&mut request).unwrap();
        assert_eq!(request, [3, 0x12, 0x34, priority]);
        assert_eq!(worker.no_timeout_cycle, 0);
    }
    // Exercise the real raw15 handshake, not an already-attached socket.
    worker.stream = None;
    drop(server);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    worker.port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut hello = [0];
        socket.read_exact(&mut hello).unwrap();
        assert_eq!(hello, [15]);
        for byte in 0..8 {
            socket.write_all(&[byte]).unwrap();
        }
    });
    worker.open_socket().unwrap();
    assert!(worker.stream.is_some());
    server.join().unwrap();
}

#[test]
fn java_update_chunk_boundaries_and_fragmented_headers_preserve_bytes() {
    for size in [499usize, 500, 501, 1000, 1001] {
        let (mut worker, rx, mut server) = fixture();
        let mut request = OnDemandRequest::new(2, 0x123);
        request.urgent = true;
        worker.pending.push(request);
        let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        for (part, bytes) in payload.chunks(500).enumerate() {
            // Original Java OnDemand:653–658 and engine encodeChunk:268–277.
            let header = [2, 1, 0x23, (size >> 8) as u8, size as u8, part as u8];
            for (index, byte) in header[..5].iter().enumerate() {
                server.write_all(&[*byte]).unwrap();
                available(&mut worker, (index + 1) as i32);
                worker.try_read().unwrap();
                assert!(rx.try_recv().is_err());
                assert_eq!(worker.part_available, 0);
            }
            server.write_all(&header[5..]).unwrap();
            if bytes.len() <= 6 {
                // Java reuses pre-header available: a short tail's read blocks.
                server.write_all(bytes).unwrap();
                available(&mut worker, (6 + bytes.len()) as i32);
                worker.try_read().unwrap();
            } else {
                available(&mut worker, 6);
                worker.try_read().unwrap();
                server.write_all(&bytes[..bytes.len() - 1]).unwrap();
                available(&mut worker, (bytes.len() - 1) as i32);
                worker.try_read().unwrap();
                assert!(rx.try_recv().is_err(), "partial chunk must not complete");
                server.write_all(&bytes[bytes.len() - 1..]).unwrap();
                available(&mut worker, bytes.len() as i32);
                worker.try_read().unwrap();
            }
            if (part + 1) * 500 < size {
                assert!(rx.try_recv().is_err());
            }
        }
        match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            WorkerMessage::Completed {
                archive,
                file,
                data,
                urgent,
                decoded,
            } => {
                assert_eq!((archive, file, urgent, decoded), (2, 0x123, true, false));
                assert_eq!(data.unwrap().as_slice(), payload);
            }
            _ => panic!("expected completed update"),
        }
        assert!(worker.pending.is_empty());
    }
}

#[test]
fn java_absent_file_urgent_completion_and_nonurgent_drop_match() {
    for urgent in [true, false] {
        let (mut worker, rx, mut server) = fixture();
        let mut request = OnDemandRequest::new(2, 0x123);
        request.urgent = urgent;
        worker.pending.push(request);
        server.write_all(&[2, 1, 0x23, 0, 0, 0]).unwrap();
        available(&mut worker, 6);
        worker.try_read().unwrap();
        assert!(worker.pending.is_empty());
        if urgent {
            assert!(matches!(
                rx.try_recv().unwrap(),
                WorkerMessage::Completed { data: None, .. }
            ));
        } else {
            assert!(rx.try_recv().is_err());
        }
    }
}

#[test]
fn java_crc_excludes_version_trailer_and_gzip_decodes_payload() {
    // CRC32("123456789") is the standard check vector, independently checked with Java CRC32.
    let mut bytes = b"123456789\x12\x34".to_vec();
    assert!(validate(0xcbf4_3926u32 as i32, 0x1234, Some(&bytes)));
    assert!(!validate(0xcbf4_3926u32 as i32, 0x1235, Some(&bytes)));
    bytes[0] ^= 1;
    assert!(!validate(0xcbf4_3926u32 as i32, 0x1234, Some(&bytes)));
    assert!(!validate(0, 0, None));
    assert!(!validate(0, 0, Some(&[0])));
    // Synthetic gzip fixture generated independently with Python gzip.compress(mtime=0).
    let gzip = [
        31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 51, 52, 50, 54, 49, 53, 51, 183, 176, 4, 0, 38, 57, 244,
        203, 9, 0, 0, 0,
    ];
    let mut versioned = gzip.to_vec();
    versioned.extend_from_slice(&[0x12, 0x34]);
    assert_eq!(gunzip(&versioned), b"123456789");
}
