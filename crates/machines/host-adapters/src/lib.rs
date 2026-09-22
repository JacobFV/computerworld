//! Explicit host capabilities. Nothing here is installed by the canonical runtime.
//! The default fixture adapter performs no IO. Real HTTP requires `native-http`.
use cw_network::{HostAdapter, HostAuthorization, NetworkError, Result};
use cw_protocol::{HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Recorded external effects can be consumed offline, in strict request order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedExchange {
    pub request: HttpRequest,
    pub response: HttpResponse,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecordedAdapter {
    remaining: VecDeque<RecordedExchange>,
}
impl RecordedAdapter {
    pub fn new(exchanges: impl IntoIterator<Item = RecordedExchange>) -> Self {
        Self {
            remaining: exchanges.into_iter().collect(),
        }
    }
    pub fn remaining(&self) -> usize {
        self.remaining.len()
    }
}
impl HostAdapter for RecordedAdapter {
    fn execute(&mut self, auth: &HostAuthorization, request: &HttpRequest) -> Result<HttpResponse> {
        validate_authorization(auth, request)?;
        let next = self
            .remaining
            .front()
            .ok_or_else(|| NetworkError::Invalid("recorded host results exhausted".into()))?;
        if &next.request != request {
            return Err(NetworkError::Denied(
                "recorded request does not match; no live fallback".into(),
            ));
        }
        if next.response.body.len() > auth.max_response_bytes {
            return Err(NetworkError::Denied(
                "recorded response exceeds policy budget".into(),
            ));
        }
        Ok(self.remaining.pop_front().unwrap().response)
    }
}
fn validate_authorization(auth: &HostAuthorization, request: &HttpRequest) -> Result<()> {
    if auth.url != request.url || auth.addresses.is_empty() {
        return Err(NetworkError::Denied(
            "request does not match explicit authorization".into(),
        ));
    }
    let url = url::Url::parse(&request.url).map_err(|e| NetworkError::Invalid(e.to_string()))?;
    if !["http", "https"].contains(&url.scheme())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(auth.port)
    {
        return Err(NetworkError::Denied(
            "invalid authorized HTTP destination".into(),
        ));
    }
    for address in &auth.addresses {
        address
            .parse::<std::net::IpAddr>()
            .map_err(|_| NetworkError::Invalid("authorized address is not an IP literal".into()))?;
    }
    if let Some(host) = url.host_str() {
        if let Ok(literal) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
            if !auth
                .addresses
                .iter()
                .any(|ip| ip.parse::<std::net::IpAddr>().ok() == Some(literal))
            {
                return Err(NetworkError::Denied(
                    "literal URL address differs from authorization".into(),
                ));
            }
        }
    }
    if request.header("host").is_some() {
        return Err(NetworkError::Denied(
            "caller cannot override authorized Host header".into(),
        ));
    }
    Ok(())
}

/// Opt-in native adapter. DNS resolution is deliberately the owner's responsibility:
/// pass all resolved addresses through Network::authorize_host before calling this.
#[cfg(all(feature = "native-http", not(target_arch = "wasm32")))]
pub struct NativeHttpAdapter {
    timeout: std::time::Duration,
}
#[cfg(all(feature = "native-http", not(target_arch = "wasm32")))]
impl NativeHttpAdapter {
    pub fn new(timeout: std::time::Duration) -> Self {
        Self {
            timeout: timeout.min(std::time::Duration::from_secs(30)),
        }
    }
}
#[cfg(all(feature = "native-http", not(target_arch = "wasm32")))]
impl HostAdapter for NativeHttpAdapter {
    fn execute(&mut self, auth: &HostAuthorization, request: &HttpRequest) -> Result<HttpResponse> {
        use std::io::Read;
        validate_authorization(auth, request)?;
        let url =
            url::Url::parse(&request.url).map_err(|e| NetworkError::Invalid(e.to_string()))?;
        let host = url
            .host_str()
            .ok_or_else(|| NetworkError::Invalid("URL has no host".into()))?;
        let addresses: Vec<_> = auth
            .addresses
            .iter()
            .map(|ip| {
                ip.parse::<std::net::IpAddr>()
                    .map(|ip| std::net::SocketAddr::new(ip, auth.port))
            })
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| NetworkError::Invalid(e.to_string()))?;
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(
                self.timeout
                    .min(std::time::Duration::from_micros(auth.timeout_us.max(1))),
            )
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|e| NetworkError::Invalid(e.to_string()))?;
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|e| NetworkError::Invalid(e.to_string()))?;
        let mut builder = client.request(method, request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .body(request.body.clone())
            .send()
            .map_err(|e| NetworkError::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_owned(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let mut body = Vec::new();
        response
            .take((auth.max_response_bytes as u64).saturating_add(1))
            .read_to_end(&mut body)
            .map_err(|e| NetworkError::Unreachable(e.to_string()))?;
        if body.len() > auth.max_response_bytes {
            return Err(NetworkError::Denied(
                "response exceeds policy byte budget".into(),
            ));
        }
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn auth() -> HostAuthorization {
        HostAuthorization {
            source: "a".into(),
            url: "https://example.test/".into(),
            addresses: vec!["192.0.2.1".into()],
            port: 443,
            deadline: 1000,
            timeout_us: 1000,
            max_response_bytes: 100,
        }
    }
    #[test]
    fn replay_never_falls_back_or_consumes_mismatch() {
        let request = HttpRequest::get("https://example.test/");
        let mut adapter = RecordedAdapter::new([RecordedExchange {
            request: request.clone(),
            response: HttpResponse::text(200, "recorded"),
        }]);
        assert!(adapter
            .execute(&auth(), &HttpRequest::get("https://other.test/"))
            .is_err());
        assert_eq!(adapter.remaining(), 1);
        assert_eq!(
            adapter.execute(&auth(), &request).unwrap().body,
            b"recorded"
        );
        assert!(adapter.execute(&auth(), &request).is_err());
    }
    #[test]
    fn refuses_host_override_and_byte_budget() {
        let mut request = HttpRequest::get("https://example.test/");
        request.headers.insert("Host".into(), "private.test".into());
        assert!(validate_authorization(&auth(), &request).is_err());
        let request = HttpRequest::get("https://example.test/");
        let mut adapter = RecordedAdapter::new([RecordedExchange {
            request: request.clone(),
            response: HttpResponse::text(200, "long"),
        }]);
        let mut auth = auth();
        auth.max_response_bytes = 1;
        assert!(adapter.execute(&auth, &request).is_err());
        assert_eq!(adapter.remaining(), 1);
    }
}

#[cfg(all(test, feature = "native-http", not(target_arch = "wasm32")))]
mod native_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };
    #[test]
    fn pins_address_preserves_host_and_does_not_follow_redirects() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let received = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
            assert!(received.contains(&format!("host: only-fixture.invalid:{port}")));
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://blocked.invalid/private\r\nContent-Length: 7\r\nConnection: close\r\n\r\nfixture").unwrap();
        });
        let request = HttpRequest::get(format!("http://only-fixture.invalid:{port}/"));
        let auth = HostAuthorization {
            source: "source".into(),
            url: request.url.clone(),
            addresses: vec!["127.0.0.1".into()],
            port,
            deadline: 3_000_000,
            timeout_us: 3_000_000,
            max_response_bytes: 128,
        };
        let result = NativeHttpAdapter::new(Duration::from_secs(3))
            .execute(&auth, &request)
            .unwrap();
        assert_eq!(result.status, 302);
        assert_eq!(result.body, b"fixture");
        assert_eq!(
            result.header("location"),
            Some("http://blocked.invalid/private")
        );
        server.join().unwrap();
    }
}
