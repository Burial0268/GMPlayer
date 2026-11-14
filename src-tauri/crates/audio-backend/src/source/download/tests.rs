use super::download_to_temp_path;
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};

#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serve `declared` bytes of header but only `sent` of body, in slow chunks.
    fn serve(declared: usize, sent: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/t.mp3", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = [0u8; 1024];
            let _ = s.read(&mut req);
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\n\r\n");
            let chunk = vec![7u8; 256 * 1024];
            let mut left = sent;
            while left > 0 {
                let n = left.min(chunk.len());
                if s.write_all(&chunk[..n]).is_err() {
                    return;
                }
                left -= n;
                std::thread::sleep(Duration::from_millis(30));
            }
        });
        url
    }

    #[test]
    fn a_large_body_is_readable_while_it_lands() {
        let total = 3 * 1024 * 1024;
        let t0 = Instant::now();
        let guard = download_to_temp_path(&serve(total, total)).unwrap();
        // Returned before the ~360 ms the body takes to send.
        assert!(t0.elapsed() < Duration::from_millis(250));
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();
        assert_eq!(src.size(), Some(total as u64));
        let mut all = Vec::new();
        src.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), total);
    }

    #[test]
    fn a_body_cut_short_fails_the_read() {
        let guard = download_to_temp_path(&serve(3 * 1024 * 1024, 1024 * 1024)).unwrap();
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();
        assert!(src.read_to_end(&mut Vec::new()).is_err());
    }

    #[test]
    fn dropping_the_guard_unblocks_a_waiting_reader() {
        let guard = download_to_temp_path(&serve(3 * 1024 * 1024, 256 * 1024)).unwrap();
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();
        let t = std::thread::spawn(move || src.read_to_end(&mut Vec::new()));
        std::thread::sleep(Duration::from_millis(300));
        drop(guard);
        assert!(t.join().unwrap().is_err());
    }
}

#[cfg(test)]
mod range_tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;

    const TOTAL: usize = 12 * 1024 * 1024;

    fn byte(i: usize) -> u8 {
        (i % 251) as u8
    }

    /// Serves `TOTAL` bytes at roughly 3 MB/s per connection. With
    /// `honor_range` off it answers every request `200` from byte 0.
    fn serve(honor_range: bool) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/t.flac", listener.local_addr().unwrap());
        let connections = Arc::new(AtomicUsize::new(0));
        let counted = connections.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { return };
                counted.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    let mut req = [0u8; 2048];
                    let n = s.read(&mut req).unwrap_or(0);
                    let req = String::from_utf8_lossy(&req[..n]).to_string();
                    let start = req
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("range: bytes=")
                                .map(str::to_string)
                        })
                        .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok())
                        .filter(|_| honor_range)
                        .unwrap_or(0);
                    let head = if honor_range && start > 0 {
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{TOTAL}\r\n\r\n",
                            TOTAL - start,
                            TOTAL - 1
                        )
                    } else {
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {TOTAL}\r\n\r\n")
                    };
                    if s.write_all(head.as_bytes()).is_err() {
                        return;
                    }
                    let mut at = start;
                    while at < TOTAL {
                        let end = (at + 128 * 1024).min(TOTAL);
                        let chunk: Vec<u8> = (at..end).map(byte).collect();
                        if s.write_all(&chunk).is_err() {
                            return;
                        }
                        at = end;
                        std::thread::sleep(Duration::from_millis(40));
                    }
                });
            }
        });
        (url, connections)
    }

    #[test]
    fn a_far_seek_is_served_by_a_second_range_request() {
        let (url, connections) = serve(true);
        let guard = download_to_temp_path(&url).unwrap();
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();

        // The whole body takes ~3.8 s at the server's pace; the tail must not.
        let at = TOTAL - 50_000;
        let t0 = Instant::now();
        src.seek(SeekFrom::Start(at as u64)).unwrap();
        let mut got = vec![0u8; 1000];
        src.read_exact(&mut got).unwrap();
        assert!(
            t0.elapsed() < Duration::from_millis(1500),
            "took {:?}",
            t0.elapsed()
        );
        assert!(got.iter().enumerate().all(|(i, b)| *b == byte(at + i)));
        assert_eq!(connections.load(Ordering::SeqCst), 2);

        // And the file still ends up whole and exact, with the writers met.
        src.seek(SeekFrom::Start(0)).unwrap();
        let mut all = Vec::new();
        src.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), TOTAL);
        assert!(all.iter().enumerate().all(|(i, b)| *b == byte(i)));
    }

    #[test]
    fn a_server_that_ignores_range_falls_back_to_waiting() {
        let (url, _) = serve(false);
        let guard = download_to_temp_path(&url).unwrap();
        let mut src = crate::source::open(crate::source::SourceLocator::Path(&guard)).unwrap();
        let at = TOTAL - 50_000;
        src.seek(SeekFrom::Start(at as u64)).unwrap();
        let mut got = vec![0u8; 1000];
        src.read_exact(&mut got).unwrap();
        assert!(got.iter().enumerate().all(|(i, b)| *b == byte(at + i)));
    }
}
