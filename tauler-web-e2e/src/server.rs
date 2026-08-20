//! A static file server for the built documentation site.
//!
//! Written out rather than pulled in because the requirement is small and specific: a
//! browser will not negotiate over the MIME type of an ES module or a wasm binary, and a
//! wrong `Content-Type` on either fails the page with an error that reads like a renderer
//! bug.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

/// A running server, stopped when dropped.
pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Serve `root` on a port the OS picks.
    pub fn start(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));

        let stop_thread = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let root = root.clone();
                        thread::spawn(move || {
                            let _ = serve_one(stream, &root);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { port, stop })
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve_one(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let target = request_line.split_whitespace().nth(1).unwrap_or("/");
    let target = target.split(['?', '#']).next().unwrap_or("/");

    let Some(path) = resolve(root, target) else {
        return respond(&mut stream, 404, "text/plain", b"not found");
    };
    match std::fs::read(&path) {
        Ok(body) => respond(&mut stream, 200, content_type(&path), &body),
        Err(_) => respond(&mut stream, 404, "text/plain", b"not found"),
    }
}

/// Map a request path to a file under `root`, refusing anything that climbs out.
///
/// A directory resolves to its `index.html`, which is what Astro's output needs.
fn resolve(root: &Path, target: &str) -> Option<PathBuf> {
    let relative = Path::new(target.trim_start_matches('/'));
    if relative
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return None;
    }
    let candidate = root.join(relative);
    if candidate.is_dir() {
        let index = candidate.join("index.html");
        return index.is_file().then_some(index);
    }
    candidate.is_file().then_some(candidate)
}

fn reason_for(status: u16) -> &'static str {
    if status == 200 {
        "OK"
    } else {
        "Not Found"
    }
}

/// `.js` and `.wasm` are the two that matter — see the module docs.
fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = reason_for(status);
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_resolves_to_its_index() {
        let dir = std::env::temp_dir().join("tauler-web-e2e-server-test/components");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("index.html"), "<h1>hi</h1>").expect("write");
        let root = dir.parent().expect("parent");
        assert_eq!(resolve(root, "/components/"), Some(dir.join("index.html")));
    }

    /// A served tree is not a sandbox by accident; it has to refuse the climb.
    #[test]
    fn a_path_climbing_out_of_the_root_is_refused() {
        assert_eq!(resolve(Path::new("/srv"), "/../etc/passwd"), None);
    }

    #[test]
    fn a_missing_file_says_not_found() {
        assert_eq!(reason_for(404), "Not Found");
    }

    #[test]
    fn modules_and_wasm_get_the_types_a_browser_insists_on() {
        assert!(content_type(Path::new("a.js")).starts_with("text/javascript"));
        assert_eq!(content_type(Path::new("a.wasm")), "application/wasm");
    }
}
