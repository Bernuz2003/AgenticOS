use super::{HttpEndpoint, HttpRequestOptions};
use crate::backend::common::response::is_timeout_error;
use std::io::{Error, ErrorKind, Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

#[test]
fn recognizes_common_socket_timeout_errors() {
    assert!(is_timeout_error(&Error::from(ErrorKind::WouldBlock)));
    assert!(is_timeout_error(&Error::from(ErrorKind::TimedOut)));
    assert!(is_timeout_error(&Error::from_raw_os_error(11)));
    assert!(is_timeout_error(&Error::from_raw_os_error(110)));
}

#[test]
fn request_json_stops_after_content_length_without_waiting_for_close() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind http listener");
    let addr = listener.local_addr().expect("listener addr");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept http client");
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).expect("read request");
        let body = r#"{"ok":true}"#;
        let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
                body.len(),
                body
            );
        stream
            .write_all(response.as_bytes())
            .expect("write response body");
        thread::sleep(Duration::from_millis(200));
    });

    let endpoint = HttpEndpoint::parse(&format!("http://{}", addr)).expect("parse endpoint");
    let response = endpoint
        .request_json_with_options(
            "GET",
            "/health",
            None,
            HttpRequestOptions {
                timeout_ms: 50,
                max_request_bytes: usize::MAX,
                max_response_bytes: usize::MAX,
                extra_headers: None,
            },
        )
        .expect("http client should not wait for socket close");

    assert_eq!(response.status_code, 200);
    assert_eq!(response.body, "{\"ok\":true}");

    server.join().expect("join http server");
}
