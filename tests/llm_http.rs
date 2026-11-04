//! Optional wire-level integration test; no external service or API key needed.
use organizer::llm::provider::{LlmProvider, LlmRequest, OpenAiCompatible, ProviderConfig};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

#[test]
#[ignore = "requires permission to bind a loopback TCP listener; run with --ignored outside restricted sandboxes"]
fn local_http_round_trip_and_redirect_do_not_expose_auth_or_follow_location() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for redirect in [false, true] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut input = BufReader::new(stream.try_clone().unwrap());
            let mut start = String::new();
            input.read_line(&mut start).unwrap();
            assert!(start.starts_with("POST /v1/chat/completions HTTP/1.1"));
            let mut length = 0;
            let mut authorized = false;
            loop {
                let mut header = String::new();
                input.read_line(&mut header).unwrap();
                if header == "\r\n" {
                    break;
                }
                let header = header.to_ascii_lowercase();
                if let Some(value) = header.strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                if header.trim() == "authorization: bearer test-token" {
                    authorized = true;
                }
            }
            assert!(authorized);
            let mut body = vec![0; length];
            input.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["response_format"]["type"], "json_object");
            assert_eq!(request["stream"], false);
            if redirect {
                write!(stream,"HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/never\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            } else {
                let response=json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"{\"schema_version\":1}"}}]}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
            }
        }
    });
    let provider = OpenAiCompatible::new(ProviderConfig {
        base_url: format!("http://{address}/v1"),
        model: "mock".into(),
        api_key: Some("test-token".into()),
        local_only: true,
        timeout: Duration::from_secs(5),
    })
    .unwrap();
    let request = LlmRequest {
        task: "tree_editor".into(),
        input: json!({"user_intent":"keep"}),
        output_schema: json!({"type":"object"}),
        repair: false,
    };
    assert_eq!(
        provider.complete_json(&request).unwrap()["schema_version"],
        1
    );
    let error = provider.complete_json(&request).unwrap_err().to_string();
    assert!(error.contains("307"));
    assert!(!error.contains("test-token"));
    server.join().unwrap();
}
