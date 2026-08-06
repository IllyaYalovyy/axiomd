//! An origin the tests can point a document at, and count what reaches it.
//!
//! "Zero implicit network" is only worth as much as the way it is checked, and the
//! only honest way is to be the server: a document that would fetch something fetches
//! it from here, and here remembers. A test asserts the *absence* of requests before
//! the reader clicks, and their exact number afterwards.
//!
//! It listens on `127.0.0.1` and a port the kernel picks, so nothing is published
//! beyond this machine and two concurrent runs cannot collide — the same reasoning
//! that kept `gtk4-broadwayd` out of the harness (`axiomd-e2e/src/display.rs`).

#![allow(dead_code)]

pub mod paper;

use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// An SVG carrying a script, so that "a remote SVG is displayed" and "it cannot do
/// anything" can be asserted about the same bytes.
pub const HOSTILE_SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"20\">\
     <script>document.querySelector('h1').textContent = 'HIJACKED'</script>\
     <rect width=\"40\" height=\"20\" fill=\"#4080ff\"/></svg>";

/// A web server on this machine, and everything it was asked for.
pub struct Origin {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
    closing: Arc<AtomicBool>,
    serving: Option<JoinHandle<()>>,
}

impl Origin {
    /// Starts serving, and returns once it is accepting: a test never races it.
    pub fn start() -> Origin {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
        let address = format!("http://{}", listener.local_addr().expect("a local address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let closing = Arc::new(AtomicBool::new(false));

        let serving = std::thread::spawn({
            let requests = requests.clone();
            let closing = closing.clone();
            move || {
                for connection in listener.incoming() {
                    if closing.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Ok(connection) = connection {
                        answer(connection, &requests);
                    }
                }
            }
        });

        Origin {
            address,
            requests,
            closing,
            serving: Some(serving),
        }
    }

    /// The address of `path` on this server, to put in a document.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.address)
    }

    /// Every path this server has been asked for, in order.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("the request log").clone()
    }
}

impl Drop for Origin {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::SeqCst);
        // The accept loop is blocked on a connection that will never come unless one
        // is made; this is that one.
        if let Some(address) = self.address.strip_prefix("http://")
            && let Ok(waking) = TcpStream::connect(address)
        {
            let _ = waking.shutdown(Shutdown::Both);
        }
        if let Some(serving) = self.serving.take() {
            let _ = serving.join();
        }
    }
}

/// Reads one request, records it, and answers it.
fn answer(mut connection: TcpStream, requests: &Arc<Mutex<Vec<String>>>) {
    let mut incoming = BufReader::new(match connection.try_clone() {
        Ok(reading) => reading,
        Err(_) => return,
    });
    let mut request = String::new();
    if incoming.read_line(&mut request).is_err() || request.trim().is_empty() {
        return;
    }
    // "GET /a.png HTTP/1.1"
    let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
    // The headers, which nothing here needs but the connection must be drained of.
    let mut header = String::new();
    while incoming.read_line(&mut header).is_ok_and(|read| read > 0) {
        if header.trim().is_empty() {
            break;
        }
        header.clear();
    }
    requests.lock().expect("the request log").push(path.clone());

    let (status, content_type, body) = if path.ends_with(".png") {
        ("200 OK", "image/png", png())
    } else if path.ends_with(".svg") {
        ("200 OK", "image/svg+xml", HOSTILE_SVG.as_bytes().to_vec())
    } else if path.ends_with(".html") {
        // Something that is not an image, answered as though it were fine: the app
        // has to notice that what came back is not a picture.
        ("200 OK", "text/html", b"<h1>not an image</h1>".to_vec())
    } else {
        ("404 Not Found", "text/plain", b"no such thing".to_vec())
    };

    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    );
    let _ = connection.write_all(head.as_bytes());
    let _ = connection.write_all(&body);
    let _ = connection.flush();
    let _ = connection.shutdown(Shutdown::Both);
}

/// A real PNG, encoded rather than pasted: a hand-written blob that turned out not to
/// decode would make "the image is displayed" pass on a broken image.
///
/// 40×20, which is the size every test that asserts a picture arrived reads back.
pub fn png() -> Vec<u8> {
    use gtk::gdk_pixbuf::{Colorspace, Pixbuf};

    let pixbuf = Pixbuf::new(Colorspace::Rgb, false, 8, 40, 20).expect("allocate a picture");
    pixbuf.fill(0x4080ffff);
    pixbuf
        .save_to_bufferv("png", &[])
        .expect("encode a picture")
        .to_vec()
}
