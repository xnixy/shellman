//! A small blocking HTTP(S) client on WinHTTP, Windows' own: TLS, proxies and certificates are
//! the system's, and no crate or async runtime is added. Any verb (CalDAV's PROPFIND and REPORT),
//! redirects handed back to the caller rather than followed (so credentials never follow a
//! redirect to another host), and a short timeout. Call it from a worker thread, never the UI.
//!
//! Requests carry the generic User-Agent `shellman` and only the headers the caller gives.
//! Nothing here logs a header or a body.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

/// How long resolving, connecting, sending and receiving may each take.
const TIMEOUT_MS: i32 = 15_000;
/// The most of a body read (calendar answers are tens of kilobytes).
const MAX_BODY: usize = 16 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    /// The URL could not be read.
    BadUrl,
    /// No connection: offline, the name did not resolve, refused, timed out, TLS failed.
    Network(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::BadUrl => write!(f, "not a valid address"),
            NetError::Network(why) => write!(f, "{why}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    /// The `Location` header of a redirect.
    pub location: Option<String>,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// A URL's parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub https: bool,
    pub host: String,
    pub port: u16,
    /// The path and query, starting with `/`.
    pub path: String,
}

/// Read `http[s]://host[:port][/path]`.
pub fn parse_url(url: &str) -> Option<Url> {
    let url = url.trim();
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return None;
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    // No credentials in URLs.
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || p.parse::<u16>().is_ok() => (h, p.parse().ok()?),
        _ => (authority, if https { 443 } else { 80 }),
    };
    let path = if path.starts_with('?') { format!("/{path}") } else { path.to_owned() };
    Some(Url { https, host: host.to_owned(), port, path })
}

/// `%`-encode a query value.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Base64, for a Basic authorization header.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn last_error(what: &str) -> NetError {
    let code = unsafe { windows::Win32::Foundation::GetLastError() }.0;
    let why = match code {
        12002 => "the server did not answer in time".to_owned(),
        12007 => "the server's name could not be found".to_owned(),
        12029 => "could not connect to the server".to_owned(),
        12030 => "the connection was cut".to_owned(),
        12175 | 12044 | 12045 | 12037 | 12038 => "the secure connection failed".to_owned(),
        _ => format!("{what} failed ({code})"),
    };
    NetError::Network(why)
}

/// Send one request and read the answer. Redirects come back as they are (see
/// [`Response::location`]).
pub fn send(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Result<Response, NetError> {
    let mut out = Vec::new();
    let (status, location) = exchange(method, url, headers, body, &mut |chunk| {
        out.extend_from_slice(chunk);
        if out.len() > MAX_BODY {
            return Err(NetError::Network("the answer was too large".into()));
        }
        Ok(())
    })?;
    Ok(Response { status, body: out, location })
}

/// GET `url` into the file `dest`, following up to five redirects, each of which `allow` must
/// accept (say, HTTPS only). At most `max` bytes; `progress(bytes so far)`. A failed or refused
/// download leaves no file. Only `Accept` is sent: no credentials, so following a redirect to
/// another host (GitHub's release downloads go to its storage host) gives nothing away.
pub fn download(url: &str, dest: &std::path::Path, max: u64, allow: &dyn Fn(&Url) -> bool, progress: &mut dyn FnMut(u64)) -> Result<(), NetError> {
    use std::io::Write;
    let mut url = url.to_owned();
    for _ in 0..6 {
        let u = parse_url(&url).ok_or(NetError::BadUrl)?;
        if !allow(&u) {
            return Err(NetError::Network(format!("refused to download from {}", u.host)));
        }
        let mut file = std::fs::File::create(dest).map_err(|e| NetError::Network(format!("cannot write {}: {e}", dest.display())))?;
        let mut got = 0u64;
        let result = exchange("GET", &url, &[("Accept", "application/octet-stream")], None, &mut |chunk| {
            got += chunk.len() as u64;
            if got > max {
                return Err(NetError::Network("the download is larger than expected".into()));
            }
            file.write_all(chunk).map_err(|e| NetError::Network(format!("cannot write {}: {e}", dest.display())))?;
            progress(got);
            Ok(())
        });
        let flushed = file.sync_all();
        drop(file);
        let fail = |e: NetError| {
            let _ = std::fs::remove_file(dest);
            Err(e)
        };
        match result {
            Err(e) => return fail(e),
            Ok((status, Some(location))) if (300..400).contains(&status) => {
                let _ = std::fs::remove_file(dest);
                url = if location.starts_with('/') { format!("{}://{}:{}{location}", if u.https { "https" } else { "http" }, u.host, u.port) } else { location };
            }
            Ok((status, _)) if (200..300).contains(&status) => {
                if let Err(e) = flushed {
                    return fail(NetError::Network(format!("cannot write {}: {e}", dest.display())));
                }
                return Ok(());
            }
            Ok((status, _)) => return fail(NetError::Network(format!("the server answered {status}"))),
        }
    }
    let _ = std::fs::remove_file(dest);
    Err(NetError::Network("too many redirects".into()))
}

/// One request; the body goes to `sink` piece by piece. Returns the status and any `Location`.
fn exchange(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&[u8]>, sink: &mut dyn FnMut(&[u8]) -> Result<(), NetError>) -> Result<(u16, Option<String>), NetError> {
    let u = parse_url(url).ok_or(NetError::BadUrl)?;
    unsafe {
        let session = Handle(WinHttpOpen(
            &HSTRING::from("shellman"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(last_error("opening WinHTTP"));
        }
        let _ = WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS);
        let connect = Handle(WinHttpConnect(session.0, &HSTRING::from(u.host.as_str()), u.port, 0));
        if connect.0.is_null() {
            return Err(last_error("connecting"));
        }
        let flags = if u.https { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let request = Handle(WinHttpOpenRequest(
            connect.0,
            &HSTRING::from(method),
            &HSTRING::from(u.path.as_str()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        ));
        if request.0.is_null() {
            return Err(last_error("opening the request"));
        }
        // Redirects are the caller's to follow; no cookies; no automatic logon with the
        // Windows account.
        let disable = (WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES).to_ne_bytes();
        let _ = WinHttpSetOption(Some(request.0), WINHTTP_OPTION_DISABLE_FEATURE, Some(&disable));
        let logon = WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH.to_ne_bytes();
        let _ = WinHttpSetOption(Some(request.0), WINHTTP_OPTION_AUTOLOGON_POLICY, Some(&logon));
        let mut head = String::new();
        for (k, v) in headers {
            head.push_str(k);
            head.push_str(": ");
            head.push_str(v);
            head.push_str("\r\n");
        }
        let head16: Vec<u16> = head.encode_utf16().collect();
        let body = body.unwrap_or(&[]);
        let sent = WinHttpSendRequest(
            request.0,
            (!head16.is_empty()).then_some(&head16[..]),
            (!body.is_empty()).then_some(body.as_ptr().cast()),
            body.len() as u32,
            body.len() as u32,
            0,
        );
        if sent.is_err() {
            return Err(last_error("sending"));
        }
        if WinHttpReceiveResponse(request.0, std::ptr::null_mut()).is_err() {
            return Err(last_error("receiving"));
        }
        let mut status = 0u32;
        let mut len = 4u32;
        let _ = WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&raw mut status).cast()),
            &mut len,
            std::ptr::null_mut(),
        );
        let location = header(request.0, WINHTTP_QUERY_LOCATION);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            if WinHttpReadData(request.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read).is_err() {
                return Err(last_error("reading"));
            }
            if read == 0 {
                break;
            }
            sink(&buf[..read as usize])?;
        }
        Ok((status as u16, location))
    }
}

unsafe fn header(request: *mut core::ffi::c_void, which: u32) -> Option<String> {
    let mut len = 0u32;
    let _ = WinHttpQueryHeaders(request, which, PCWSTR::null(), None, &mut len, std::ptr::null_mut());
    if len == 0 {
        return None;
    }
    let mut buf = vec![0u16; len as usize / 2 + 1];
    let mut bytes = (buf.len() * 2) as u32;
    WinHttpQueryHeaders(request, which, PCWSTR::null(), Some(buf.as_mut_ptr().cast()), &mut bytes, std::ptr::null_mut()).ok()?;
    let n = (bytes as usize / 2).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').to_owned())
}

/// GET a URL and read it as text; an error for anything but 2xx.
pub fn get_text(url: &str) -> Result<String, NetError> {
    let r = send("GET", url, &[("Accept", "application/json")], None)?;
    if r.ok() {
        Ok(r.text())
    } else {
        Err(NetError::Network(format!("the server answered {}", r.status)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let u = parse_url("https://caldav.fastmail.com/").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.path.as_str()), (true, "caldav.fastmail.com", 443, "/"));
        let u = parse_url("http://127.0.0.1:8765/dav/x?y=1").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.path.as_str()), (false, "127.0.0.1", 8765, "/dav/x?y=1"));
        assert_eq!(parse_url("https://h.example").unwrap().path, "/");
        assert_eq!(parse_url("https://h.example?q=1").unwrap().path, "/?q=1");
        assert!(parse_url("ftp://h").is_none());
        assert!(parse_url("https://user:pw@h.example/").is_none());
        assert!(parse_url("https://h.example:notaport/").is_none());
    }

    /// A real request; run with `--ignored --nocapture` (needs the network).
    #[test]
    #[ignore]
    fn live() {
        for url in ["https://www.google.com/", "https://api.open-meteo.com/v1/forecast?latitude=51.5&longitude=-0.12&current=temperature_2m"] {
            let t = std::time::Instant::now();
            let r = send("GET", url, &[], None);
            println!("{url}: {:?} in {:?}", r.map(|r| (r.status, r.body.len())), t.elapsed());
        }
    }

    #[test]
    fn encodings() {
        assert_eq!(encode("São Paulo"), "S%C3%A3o%20Paulo");
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b""), "");
    }
}
