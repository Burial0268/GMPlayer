//! One HTTP transfer, resumable and interruptible.
//!
//! reqwest over hyper, on the runtime the app already has. Async rather than
//! blocking because a download queue is the wrong shape for a blocking client: a
//! multi-megabyte transfer would hold a thread for its whole life, several at a
//! time, and cancelling one would mean waiting for the next chunk either way.
//!
//! # One client, and why pooling is safe here
//!
//! [`client`] is built once. That is not a micro-optimisation — a reqwest client
//! builds a rustls config and loads the platform trust store, which is far more
//! expensive than the handshake it saves.
//!
//! The rule in `AGENTS.md` about *not* pooling applies to `ureq` on the playback
//! source path, where a reused connection handed over an undrained body and a
//! track decoded as the previous track's duration plus its own. hyper cannot do
//! that: a connection whose body was not read to the end is dropped instead of
//! being returned to the pool, so the failure that rule exists to prevent is
//! structurally impossible on this stack.
//!
//! # `Accept-Encoding: identity` is load-bearing
//!
//! `tauri-plugin-http` enables reqwest's `gzip`/`brotli` on the *same* 0.12, and
//! Cargo unifies features — so transparent decompression is compiled in here
//! whether this crate asks for it or not. A decompressed body breaks two things
//! at once: the byte count stops matching `Content-Length`, and a `Range` resume
//! addresses offsets in the encoded stream that no longer correspond to what was
//! written. Asking for `identity` explicitly settles it at the protocol level
//! instead of depending on a sibling's feature list.
//!
//! # One range per stream, one file per range
//!
//! [`stream`] moves *one* [`Segment`] — a place in the remote resource, how many
//! bytes of it this file already holds, and how many it should end up with — into
//! *one* append-only handle. A whole-file transfer is the degenerate case
//! (`start: 0`, `want: None`), which is why the single-stream wire behaviour is
//! byte-for-byte what it was: no `Range` header at all on a fresh download, an
//! open-ended one on a resume.
//!
//! Splitting a download across several of these is what [`split`] and
//! [`segments_for`] decide, and `sink` gives each segment its own file so that
//! every one of them stays append-only and its length stays its own progress
//! record. Nothing here writes at an offset.

use std::sync::OnceLock;
use std::time::Duration;

use tokio::io::AsyncWriteExt;

/// How the transfer ended.
#[derive(Debug)]
pub enum Transfer {
    /// Every byte landed. `total` is the finished file's size.
    Complete { total: u64 },
    /// The caller asked it to stop. The partial file is intact and resumable.
    Interrupted { written: u64 },
    /// A `Range` request came back `200 OK` instead of `206`, i.e. the server is
    /// sending the whole file from the start.
    ///
    /// Reported rather than handled here because the fix is not this layer's: the
    /// partial file has to be discarded and reopened, and the handle given to
    /// [`stream`] is append-only by design (see `sink`), so it cannot be rewound.
    RangeRefused,
}

/// A contiguous stretch of the remote resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u64,
    pub len: u64,
}

/// One stream's worth of work: which bytes, and how far along the file is.
#[derive(Debug, Clone, Copy)]
pub struct Segment<'a> {
    pub url: &'a str,
    /// Where this file's bytes begin in the remote resource.
    pub start: u64,
    /// Bytes already in the file. The request resumes at `start + have`.
    pub have: u64,
    /// How many bytes the file should end up holding. `None` means "to the end of
    /// the resource", which is only ever the single-stream case — a segment of a
    /// split download always knows its length, because that is what makes its own
    /// file's length a complete progress record.
    pub want: Option<u64>,
}

/// The shared client. Built on first use and kept for the process.
fn client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                // A *read* timeout, not a total one: a lossless file over a slow
                // connection legitimately takes minutes, and a whole-request
                // budget would abort exactly the transfers that most need
                // resuming. This bounds silence instead of duration.
                .read_timeout(Duration::from_secs(60))
                .build()
                .map_err(|e| format!("could not build the download client: {e}"))
        })
        .as_ref()
        .map_err(|err| err.clone())
}

/// The `Range` header for `segment`, or `None` when the whole resource is being
/// asked for from byte zero.
///
/// A segment asks for a **closed** range so that a well-behaved server stops on
/// its own; the byte budget in [`stream`] is what catches one that does not.
fn request_range(segment: &Segment<'_>) -> Option<String> {
    let from = segment.start + segment.have;
    match segment.want {
        Some(want) => Some(format!("bytes={from}-{}", segment.start + want - 1)),
        None if from > 0 => Some(format!("bytes={from}-")),
        None => None,
    }
}

/// Stream `segment` into `file`, continuing from what the file already holds.
///
/// `on_progress` is called per chunk with (bytes in this file, bytes this file
/// should end up with). Throttle whatever it does — the queue publishes at most
/// one event per whole percent, aggregated across every segment of the task.
///
/// `stop` is polled per chunk. A closure rather than a flag because a split
/// transfer has two reasons to stop: the user paused it, or a sibling segment
/// failed and there is no point paying for the rest of this one.
pub async fn stream(
    segment: &Segment<'_>,
    file: &mut tokio::fs::File,
    stop: &impl Fn() -> bool,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<Transfer, String> {
    // A segment whose file is already full needs no request at all — which is
    // what resuming a split download that finished but never committed consists
    // of, and what a zero-length span degenerates to.
    if let Some(want) = segment.want {
        if segment.have >= want {
            return Ok(Transfer::Complete { total: segment.have });
        }
    }
    let asked = segment.start + segment.have;
    let range = request_range(segment);
    let mut request = client()?
        .get(segment.url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if let Some(range) = &range {
        request = request.header(reqwest::header::RANGE, range);
    }

    let response = request.send().await.map_err(describe)?;
    let status = response.status();
    if !status.is_success() {
        // A signed Netease URL that has expired arrives as 403, and the queue must
        // be able to tell that from a transport failure: the first is fixed by
        // resolving again, the second by retrying the same URL.
        return Err(format!("http {}", status.as_u16()));
    }
    if range.is_some() && status == reqwest::StatusCode::OK {
        return Ok(Transfer::RangeRefused);
    }
    // A 206 that starts somewhere other than where it was asked to would still be
    // *appended*, i.e. written silently at the offset we asked for — and for a
    // split download the wrong place is inside a neighbouring segment.
    if let Some((start, _)) = content_range(&response) {
        if start != asked {
            return Err(format!("the server answered from {start}, not {asked}"));
        }
    }
    let expected = segment.want.or_else(|| total_size(&response, asked));

    let mut written = segment.have;
    on_progress(written, expected);

    let mut response = response;
    loop {
        if stop() {
            file.flush().await.map_err(|e| e.to_string())?;
            return Ok(Transfer::Interrupted { written });
        }
        let Some(chunk) = response.chunk().await.map_err(describe)? else {
            break;
        };
        // A server that ignored the closed range would otherwise overrun this
        // segment and write over bytes its neighbour is going to contribute.
        let chunk = match segment.want {
            Some(want) if written + chunk.len() as u64 > want => {
                chunk.slice(..(want - written) as usize)
            }
            _ => chunk,
        };
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        written += chunk.len() as u64;
        on_progress(written, expected);
        if segment.want == Some(written) {
            // Stop reading rather than draining the rest. hyper drops a connection
            // whose body was not read to the end instead of returning it to the
            // pool, so abandoning one here cannot hand stale bytes to a later
            // request — see the note at the top of this module.
            break;
        }
    }

    file.flush().await.map_err(|e| e.to_string())?;

    // A body that stopped short of a declared length is a truncated transfer, not
    // a finished one. Reporting it as complete would publish the `.part` under the
    // real name and put an unplayable row in the library; as an error the partial
    // file survives and the retry resumes from where it stopped.
    if let Some(expected) = expected {
        if written < expected {
            return Err(format!("connection closed at {written} of {expected} bytes"));
        }
    }
    Ok(Transfer::Complete { total: written })
}

/// The finished file's size, if the server said enough to know it.
///
/// `Content-Range` first because it states the total directly; on a 206
/// `Content-Length` is only the length of *this* range, so adding the offset is
/// the equivalent. Absent both (a chunked response) the size is unknown and the
/// UI shows an indeterminate transfer.
fn total_size(response: &reqwest::Response, offset: u64) -> Option<u64> {
    if let Some((_, Some(total))) = content_range(response) {
        return Some(total);
    }
    response.content_length().map(|length| offset + length)
}

/// `Content-Range` as (first byte sent, total size if stated).
fn content_range(response: &reqwest::Response) -> Option<(u64, Option<u64>)> {
    response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)
}

/// Parse `bytes <first>-<last>/<total>`.
///
/// `None` for anything else, including the `bytes */<total>` a 416 carries and any
/// unit other than bytes — both mean "this header cannot confirm the offset", and
/// the caller treats an absent confirmation as one it cannot check rather than as
/// a failure.
fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let (unit, rest) = value.trim().split_once(' ')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (range, total) = match rest.split_once('/') {
        Some((range, total)) => (range, total.trim()),
        None => (rest, ""),
    };
    let first = range.trim().split('-').next()?.trim().parse::<u64>().ok()?;
    Some((first, total.parse::<u64>().ok()))
}

// ── Splitting a download across streams ──────────────────────────

/// Below this a file is not worth splitting: every extra stream costs a
/// handshake, and a transfer this small is over before they have paid for
/// themselves.
pub const MIN_SEGMENTED_BYTES: u64 = 4 * 1024 * 1024;

/// No segment shorter than this, so a 5 MiB file becomes two streams rather than
/// eight — past a point a segment is smaller than the request that asks for it.
pub const MIN_SEGMENT_BYTES: u64 = 2 * 1024 * 1024;

/// How many streams to split a `size`-byte download across.
///
/// Deliberately a function of the size and the setting **only**, never of how many
/// connections happen to be free: the split decides the part filenames, and a
/// resume that computed a different one would orphan the bytes already on disk.
/// The connection budget bounds how many of the segments run *at once* instead —
/// see `queue::run_segments`.
///
/// `size == 0` is the server not having declared one, which is the same answer as
/// "too small to split": there is nothing to divide.
pub fn segments_for(size: u64, wanted: usize) -> usize {
    if wanted <= 1 || size < MIN_SEGMENTED_BYTES {
        return 1;
    }
    let by_size = (size / MIN_SEGMENT_BYTES).max(1) as usize;
    wanted.min(by_size).clamp(1, super::config::MAX_SEGMENTS)
}

/// Contiguous, exhaustive spans covering `size`.
///
/// The remainder goes to the *first* segments rather than the last. The tail
/// segment is the one whose bytes are concatenated last, so leaving it the long
/// one would put the extra bytes exactly where they delay the commit.
pub fn split(size: u64, n: usize) -> Vec<Span> {
    let n = n.max(1) as u64;
    let base = size / n;
    let remainder = size % n;
    let mut spans = Vec::with_capacity(n as usize);
    let mut start = 0;
    for index in 0..n {
        let len = base + u64::from(index < remainder);
        spans.push(Span { start, len });
        start += len;
    }
    spans
}

/// Turn a reqwest error into something short enough for a task row.
///
/// `reqwest::Error`'s own `Display` chains sources and includes the URL, which for
/// a signed Netease URL is a few hundred characters of query string in the UI and
/// in the log.
fn describe(err: reqwest::Error) -> String {
    if err.is_timeout() {
        return "the connection went quiet".to_string();
    }
    if err.is_connect() {
        return "could not connect".to_string();
    }
    if let Some(status) = err.status() {
        return format!("http {}", status.as_u16());
    }
    err.without_url().to_string()
}

/// Whether a failure is worth retrying with the *same* URL.
///
/// A signed URL that has expired answers 403, and retrying it is guaranteed to
/// fail the same way — the queue re-resolves instead. Anything transport-shaped is
/// worth another attempt.
pub fn is_url_stale(error: &str) -> bool {
    error.contains("http 403") || error.contains("http 401") || error.contains("http 410")
}

/// Cap on a fetch through [`bytes`]. Cover art, not audio.
const MAX_SMALL_BYTES: usize = 8 * 1024 * 1024;

/// Fetch a small resource whole — cover art.
///
/// Bounded rather than streamed because the caller wants the bytes in memory to
/// hand to the sink, and an unbounded read of a URL that came off the wire is how
/// "save the album art" becomes an allocation failure.
pub async fn bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = client()?.get(url).send().await.map_err(describe)?;
    if !response.status().is_success() {
        return Err(format!("http {}", response.status().as_u16()));
    }
    let mut response = response;
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(describe)? {
        if out.len() + chunk.len() > MAX_SMALL_BYTES {
            return Err("the image is larger than the cover-art cap".to_string());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_url_is_told_apart_from_a_transport_failure() {
        assert!(is_url_stale("http 403"));
        assert!(!is_url_stale("http 500"));
        assert!(!is_url_stale("connection closed at 10 of 20 bytes"));
    }

    #[test]
    fn the_client_builds_on_this_platform() {
        // Cheap, and it is the one failure that would turn every download into the
        // same opaque error: a rustls config that cannot load the platform trust
        // store.
        assert!(client().is_ok());
    }

    #[test]
    fn a_content_range_is_read_for_both_its_offset_and_its_total() {
        assert_eq!(parse_content_range("bytes 100-199/1000"), Some((100, Some(1000))));
        assert_eq!(parse_content_range("bytes 0-0/1"), Some((0, Some(1))));
        // An unknown total is still a usable confirmation of the offset.
        assert_eq!(parse_content_range("bytes 500-999/*"), Some((500, None)));
        // Neither a 416's unsatisfiable form nor a different unit can confirm one.
        assert_eq!(parse_content_range("bytes */1000"), None);
        assert_eq!(parse_content_range("items 0-9/50"), None);
    }

    #[test]
    fn a_split_is_contiguous_and_exhaustive() {
        for (size, n) in [(4u64, 4usize), (10, 3), (1, 4), (1_000_003, 7)] {
            let spans = split(size, n);
            assert_eq!(spans.len(), n);
            assert_eq!(spans[0].start, 0);
            for pair in spans.windows(2) {
                assert_eq!(pair[0].start + pair[0].len, pair[1].start);
            }
            let last = spans[spans.len() - 1];
            assert_eq!(last.start + last.len, size);
            // The remainder rides at the front, never on the tail segment.
            assert!(last.len <= spans[0].len);
        }
    }

    #[test]
    fn a_download_is_only_split_when_it_is_worth_splitting() {
        // A size the server never declared is nothing to divide.
        assert_eq!(segments_for(0, 4), 1);
        assert_eq!(segments_for(MIN_SEGMENTED_BYTES - 1, 4), 1);
        assert_eq!(segments_for(64 * 1024 * 1024, 1), 1);
        // Four asked for, but there is only room for two segments of the minimum.
        assert_eq!(segments_for(5 * 1024 * 1024, 4), 2);
        assert_eq!(segments_for(64 * 1024 * 1024, 4), 4);
        assert_eq!(
            segments_for(64 * 1024 * 1024, 99),
            crate::download::config::MAX_SEGMENTS
        );
    }

    #[test]
    fn a_whole_file_download_asks_for_no_range_at_all() {
        let whole = Segment { url: "u", start: 0, have: 0, want: None };
        assert_eq!(request_range(&whole), None);
        // A single-stream resume stays open-ended, exactly as it was.
        let resumed = Segment { url: "u", start: 0, have: 10, want: None };
        assert_eq!(request_range(&resumed).as_deref(), Some("bytes=10-"));
        // A segment asks closed, so an honest server stops at its own end.
        let segment = Segment { url: "u", start: 100, have: 5, want: Some(50) };
        assert_eq!(request_range(&segment).as_deref(), Some("bytes=105-149"));
    }

    // ── Over a real socket ───────────────────────────────────────
    //
    // A hand-rolled server rather than a dependency, because what has to be
    // exercised is how this module reacts to a *misbehaving* one: a 200 where a 206
    // was asked for, a 206 from an offset nobody asked for, a range whose end is
    // ignored. Those are precisely the answers a correct server will not give.

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Behaviour {
        Honest,
        /// Answers every request with the whole body and a 200.
        NoRanges,
        /// Honours the start and ignores the end, sending to the end of the body.
        IgnoresTheEnd,
        /// Answers from an offset nobody asked for.
        WrongOffset,
    }

    /// Deterministic and non-repeating, so a segment written at the wrong offset
    /// cannot pass by accident.
    fn payload(len: usize) -> Vec<u8> {
        (0..len)
            .map(|index| (index as u64).wrapping_mul(2_654_435_761) as u8)
            .collect()
    }

    fn joined(dir: &std::path::Path, parts: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for index in 0..parts {
            let part = std::fs::read(dir.join(format!("part{index}"))).expect("a part file");
            out.extend_from_slice(&part);
        }
        out
    }

    /// Start a server on a loopback port and return its base URL.
    fn serve(payload: Vec<u8>, behaviour: Behaviour) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let url = format!("http://{}/song", listener.local_addr().expect("the bound port"));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                // A client that clamped a range and walked away leaves this write
                // half-done, which is one of the things under test.
                let _ = answer(&mut stream, &payload, behaviour);
            }
        });
        url
    }

    fn answer(
        stream: &mut std::net::TcpStream,
        payload: &[u8],
        behaviour: Behaviour,
    ) -> std::io::Result<()> {
        use std::io::{BufRead, BufReader, Write};

        let total = payload.len() as u64;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut asked = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                return Ok(());
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("range") {
                    asked = requested_range(value.trim(), total);
                }
            }
        }

        let (status, first, last) = match (asked, behaviour) {
            (None, _) | (Some(_), Behaviour::NoRanges) => (200, 0, total.saturating_sub(1)),
            (Some((first, _)), Behaviour::IgnoresTheEnd) => (206, first, total.saturating_sub(1)),
            (Some((first, last)), Behaviour::WrongOffset) => (206, first + 1, last),
            (Some((first, last)), Behaviour::Honest) => (206, first, last),
        };
        let body = &payload[first as usize..=last as usize];
        let reason = if status == 206 { "Partial Content" } else { "OK" };
        let mut head = format!("HTTP/1.1 {status} {reason}\r\nAccept-Ranges: bytes\r\n");
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        if status == 206 {
            head.push_str(&format!("Content-Range: bytes {first}-{last}/{total}\r\n"));
        }
        head.push_str("Connection: close\r\n\r\n");
        stream.write_all(head.as_bytes())?;
        stream.write_all(body)?;
        stream.flush()
    }

    fn requested_range(value: &str, total: u64) -> Option<(u64, u64)> {
        let (first, last) = value.strip_prefix("bytes=")?.split_once('-')?;
        let first = first.trim().parse::<u64>().ok()?;
        let last = match last.trim() {
            "" => total.saturating_sub(1),
            text => text.parse::<u64>().ok()?,
        };
        Some((first, last.min(total.saturating_sub(1))))
    }

    /// Move `span` into `path`, appending, exactly as the queue's lanes do.
    ///
    /// `stop_at` interrupts the transfer once that many bytes are in the file, which
    /// is what a pause looks like from in here.
    async fn move_span(
        url: &str,
        span: Span,
        unsplit: bool,
        path: &std::path::Path,
        stop_at: Option<u64>,
    ) -> Result<Transfer, String> {
        let handle = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let have = handle.metadata().map_err(|e| e.to_string())?.len();
        let mut file = tokio::fs::File::from_std(handle);
        let written = std::cell::Cell::new(have);
        let stop = || matches!(stop_at, Some(limit) if written.get() >= have + limit);
        let segment = Segment {
            url,
            start: span.start,
            have,
            want: (!unsplit).then_some(span.len),
        };
        let result = stream(&segment, &mut file, &stop, |now, _| written.set(now)).await;
        let _ = file.flush().await;
        result
    }

    #[test]
    fn a_split_transfer_reassembles_the_original_bytes() {
        // Not a round size: the remainder has to land in the right segment or the
        // concatenation is off by a few bytes in the middle.
        let bytes = payload(100_003);
        let url = serve(bytes.clone(), Behaviour::Honest);
        let dir = tempfile::tempdir().expect("tempdir");
        let spans = split(bytes.len() as u64, 4);
        tauri::async_runtime::block_on(async {
            for (index, span) in spans.iter().enumerate() {
                let path = dir.path().join(format!("part{index}"));
                let outcome = move_span(&url, *span, false, &path, None)
                    .await
                    .expect("the segment");
                assert!(matches!(outcome, Transfer::Complete { total } if total == span.len));
            }
        });
        assert_eq!(joined(dir.path(), spans.len()), bytes);
    }

    #[test]
    fn an_interrupted_segment_resumes_where_it_stopped() {
        // Large enough that a segment cannot arrive in one chunk, which is what makes
        // the interruption land mid-body rather than after the last byte.
        let bytes = payload(4 * 1024 * 1024);
        let url = serve(bytes.clone(), Behaviour::Honest);
        let dir = tempfile::tempdir().expect("tempdir");
        let spans = split(bytes.len() as u64, 4);
        let span = spans[1];
        let path = dir.path().join("part1");
        tauri::async_runtime::block_on(async {
            let first = move_span(&url, span, false, &path, Some(1))
                .await
                .expect("the first pass");
            assert!(matches!(first, Transfer::Interrupted { .. }));
            let stopped = std::fs::metadata(&path).expect("the part file").len();
            assert!(stopped > 0 && stopped < span.len, "stopped at {stopped}");

            // The file's own length is the only progress record there is.
            let second = move_span(&url, span, false, &path, None)
                .await
                .expect("the second pass");
            assert!(matches!(second, Transfer::Complete { total } if total == span.len));
        });
        let start = span.start as usize;
        let got = std::fs::read(&path).expect("the part file");
        assert_eq!(got, bytes[start..start + span.len as usize]);
    }

    #[test]
    fn a_server_that_will_not_serve_a_range_is_reported_rather_than_written() {
        let bytes = payload(100_003);
        let url = serve(bytes.clone(), Behaviour::NoRanges);
        let dir = tempfile::tempdir().expect("tempdir");
        let spans = split(bytes.len() as u64, 4);
        let path = dir.path().join("part1");
        tauri::async_runtime::block_on(async {
            let outcome = move_span(&url, spans[1], false, &path, None)
                .await
                .expect("a report, not an error");
            assert!(matches!(outcome, Transfer::RangeRefused));
        });
        // Nothing was appended, which is the whole point of reporting it: the caller
        // has to throw the file away and start over unsplit.
        assert_eq!(std::fs::metadata(&path).expect("the part file").len(), 0);

        // And unsplit, the same server is a perfectly ordinary download.
        let whole = dir.path().join("whole");
        let span = Span { start: 0, len: bytes.len() as u64 };
        tauri::async_runtime::block_on(async {
            let outcome = move_span(&url, span, true, &whole, None)
                .await
                .expect("the whole file");
            assert!(matches!(outcome, Transfer::Complete { total } if total == span.len));
        });
        assert_eq!(std::fs::read(&whole).expect("the file"), bytes);
    }

    #[test]
    fn a_server_that_ignores_the_end_of_a_range_cannot_overrun_its_segment() {
        let bytes = payload(100_003);
        let url = serve(bytes.clone(), Behaviour::IgnoresTheEnd);
        let dir = tempfile::tempdir().expect("tempdir");
        let spans = split(bytes.len() as u64, 4);
        let span = spans[1];
        let path = dir.path().join("part1");
        tauri::async_runtime::block_on(async {
            let outcome = move_span(&url, span, false, &path, None)
                .await
                .expect("the segment");
            assert!(matches!(outcome, Transfer::Complete { total } if total == span.len));
        });
        let start = span.start as usize;
        let got = std::fs::read(&path).expect("the part file");
        // Anything past the span would be bytes the *next* part is going to
        // contribute, spliced into the middle of an otherwise plausible file.
        assert_eq!(got, bytes[start..start + span.len as usize]);
    }

    #[test]
    fn a_server_answering_from_the_wrong_offset_is_refused() {
        let bytes = payload(100_003);
        let url = serve(bytes.clone(), Behaviour::WrongOffset);
        let dir = tempfile::tempdir().expect("tempdir");
        let spans = split(bytes.len() as u64, 4);
        let path = dir.path().join("part1");
        tauri::async_runtime::block_on(async {
            let err = move_span(&url, spans[1], false, &path, None)
                .await
                .expect_err("a refusal");
            assert!(err.contains("answered from"), "{err}");
        });
        assert_eq!(std::fs::metadata(&path).expect("the part file").len(), 0);
    }
}
