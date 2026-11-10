//! Serving a track's file, with `Range`.

use std::io::SeekFrom;
use std::path::Path;

use futures_util::TryStreamExt;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::Frame;
use hyper::header::{HeaderValue, ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE};
use hyper::{Response, StatusCode};
use rpm_peer::http::{empty, status, Body};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

/// What is read from the disk in one go while serving audio.
const COPY_BUFFER: usize = 128 * 1024;

/// The content type for a file, by its extension. The same list as `ContentTypes` in `RPM.Core`.
pub fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" | "m4b" | "mp4" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "wav" => "audio/wav",
        "aiff" | "aif" => "audio/aiff",
        "wma" => "audio/x-ms-wma",
        "ape" => "audio/x-ape",
        "wv" => "audio/x-wavpack",
        _ => "application/octet-stream",
    }
}

/// The first and last byte asked for, or `None` for the whole file. The same rules as `ByteRange` in
/// `RPM.Core`: one range only, `bytes=a-b`, `bytes=a-` or `bytes=-n`. Anything else is the whole file.
pub fn parse_range(header: Option<&str>, length: u64) -> Option<(u64, u64)> {
    if length == 0 {
        return None;
    }
    let text = header?.trim();
    let (unit, spec) = text.split_at_checked(6)?;
    if !unit.eq_ignore_ascii_case("bytes=") {
        return None;
    }
    let spec = spec.trim();
    if spec.contains(',') {
        return None;
    }
    let (from, to) = spec.split_once('-')?;
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() {
        let wanted: u64 = to.parse().ok().filter(|&n| n > 0)?;
        return Some((length.saturating_sub(wanted), length - 1));
    }
    let begin: u64 = from.parse().ok()?;
    if begin >= length {
        return None;
    }
    if to.is_empty() {
        return Some((begin, length - 1));
    }
    let end: u64 = to.parse().ok()?;
    if end < begin {
        return None;
    }
    Some((begin, end.min(length - 1)))
}

/// The file at `path`, whole or the range asked for. 404 when it cannot be opened.
pub async fn serve_file(path: &Path, range: Option<&str>, head_only: bool) -> Response<Body> {
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return status(StatusCode::NOT_FOUND);
    };
    let Ok(meta) = file.metadata().await else {
        return status(StatusCode::NOT_FOUND);
    };
    let length = meta.len();
    let window = parse_range(range, length);
    let (code, start, count) = match window {
        Some((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
        None => (StatusCode::OK, 0, length),
    };

    let body = if head_only {
        empty()
    } else {
        if start > 0 && file.seek(SeekFrom::Start(start)).await.is_err() {
            return status(StatusCode::NOT_FOUND);
        }
        let stream = ReaderStream::with_capacity(file.take(count), COPY_BUFFER).map_ok(Frame::data);
        StreamBody::new(stream).boxed()
    };

    let mut response = Response::new(body);
    *response.status_mut() = code;
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type(path)));
    headers.insert(CONTENT_LENGTH, HeaderValue::from(count));
    headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Some((start, end)) = window {
        if let Ok(value) = HeaderValue::from_str(&format!("bytes {start}-{end}/{length}")) {
            headers.insert(CONTENT_RANGE, value);
        }
    }
    response
}
