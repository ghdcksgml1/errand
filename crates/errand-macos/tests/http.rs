//! Real URLSession callbacks against loopback fixtures; no external service or credential is used.
#![cfg(target_os = "macos")]
use errand_core::ports::{HTTP_BODY_LIMIT, HttpClient, HttpFailure, HttpMethod, HttpRequest, HttpResponse};
use errand_macos::http::UrlSession;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn request(url: String) -> HttpRequest {
    HttpRequest { method: HttpMethod::Get, url, headers: vec![("Authorization".into(), "Bearer synthetic-fixture".into())], body: None, timeout: Duration::from_secs(5) }
}
fn server(serve: impl FnOnce(TcpStream) + Send + 'static) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/fixture", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        socket.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            socket.read_exact(&mut byte).unwrap();
            headers.extend(byte);
            assert!(headers.len() < 8192);
        }
        serve(socket);
    });
    (url, handle)
}
fn answer(wire: Vec<u8>) -> Result<HttpResponse, HttpFailure> {
    let (url, server) = server(move |mut socket| { let _ = socket.write_all(&wire); });
    let result = UrlSession.send(&request(url));
    server.join().unwrap();
    result
}
/// The server deliberately withholds EOF (and a chunk terminator, where relevant). Rejecting only after
/// the complete response, or relying on a network timeout, must fail this test.
fn rejects_before_finish(wire: Vec<u8>) {
    let (ready, receiving) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let (url, server) = server(move |mut socket| {
        let _ = socket.write_all(&wire); // cancellation may stop an oversized write
        ready.send(()).unwrap();
        let _ = held.recv_timeout(Duration::from_secs(10));
    });
    let (sent, received) = mpsc::channel();
    let client = thread::spawn(move || { let _ = sent.send(UrlSession.send(&request(url))); });
    receiving.recv_timeout(Duration::from_secs(6)).unwrap();
    let result = received.recv_timeout(Duration::from_secs(2));
    let _ = release.send(());
    server.join().unwrap();
    client.join().unwrap();
    assert_eq!(result.expect("must cancel before the server finishes, without waiting for the five-second timeout"), Err(HttpFailure));
}

#[test]
fn normal_and_exact_limit_responses_keep_status_body_and_retry_after() {
    for size in [0, 17, HTTP_BODY_LIMIT] {
        let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nRetry-After: 42\r\nConnection: close\r\n\r\n").into_bytes();
        wire.extend(vec![b'x'; size]);
        let response = answer(wire).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, vec![b'x'; size]);
        assert_eq!(response.retry_after.as_deref(), Some("42"));
    }
}

#[test]
fn oversized_content_length_is_refused_before_the_large_body_arrives() {
    // CFNetwork may defer delivering headers until the first body byte. The other 1 MiB is withheld.
    rejects_before_finish(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\nx", HTTP_BODY_LIMIT + 1).into_bytes());
}

#[test]
fn oversized_chunked_body_is_cancelled_before_the_final_chunk() {
    let size = HTTP_BODY_LIMIT + 1;
    let mut wire = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{size:X}\r\n").into_bytes();
    wire.extend(vec![b'x'; size]);
    wire.extend_from_slice(b"\r\n");
    rejects_before_finish(wire);
}

#[test]
fn oversized_unknown_length_error_is_cancelled_before_eof() {
    let mut wire = b"HTTP/1.1 500 Error\r\nConnection: close\r\n\r\n".to_vec();
    wire.extend(vec![b'x'; HTTP_BODY_LIMIT + 1]);
    rejects_before_finish(wire);
}

#[test]
fn compressed_response_is_limited_after_decoding() {
    // gzip of HTTP_BODY_LIMIT + 1 repeated bytes, with mtime zero; about 1 KiB on the wire.
    let gzip = include_bytes!("fixtures/oversized-response.gz");
    assert!(gzip.len() < 2048);
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", gzip.len()).into_bytes();
    wire.extend_from_slice(gzip);
    assert_eq!(answer(wire), Err(HttpFailure));
}

#[test]
fn bearer_credentials_never_follow_redirects() {
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let wire = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", destination.local_addr().unwrap());
    assert_eq!(answer(wire.into_bytes()).unwrap().status, 302);
    assert_eq!(destination.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}
