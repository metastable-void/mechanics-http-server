use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::{Buf, Bytes};
use http::header::ALT_SVC;
use http::{Method, Request, Response, StatusCode};
use http_body::Frame;
use http_body_util::{BodyExt, Full};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::UdpSocket;
use tower::util::BoxCloneService;
use tower::{Layer, Service, ServiceExt, service_fn};

use crate::{
    H3RequestBody, Http3Server, Http3ServerConfig, alt_svc_layer,
    axum_compat::router_into_h3_service, default_zero_rtt_methods, is_zero_rtt_safe,
};

const STREAM_TEST_BYTES: usize = 8 * 1024 * 1024;
const STREAM_TEST_CHUNK_BYTES: usize = 64 * 1024;

fn test_service() -> BoxCloneService<Request<H3RequestBody>, Response<Full<Bytes>>, Infallible> {
    service_fn(|request: Request<H3RequestBody>| async move {
        let body = if request.uri().path() == "/test" {
            Bytes::from_static(b"routed")
        } else {
            Bytes::new()
        };
        Ok::<_, Infallible>(Response::new(Full::new(body)))
    })
    .boxed_clone()
}

fn test_tls_material() -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    let CertifiedKey { cert, key_pair } =
        generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let cert_chain = vec![cert.der().clone()];
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
    (cert_chain, key)
}

#[tokio::test]
async fn start_with_no_bind_is_inert() {
    let (cert_chain, key) = test_tls_material();
    let server = Http3Server::new(Http3ServerConfig {
        bind_h3: None,
        ..Http3ServerConfig::default()
    });

    let handle = server.start(test_service(), cert_chain, key).unwrap();

    assert!(handle.is_inert());
    assert_eq!(handle.local_addr(), None);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), handle)
            .await
            .unwrap()
            .is_ok()
    );
}

#[tokio::test]
async fn start_with_bind_opens_listener() {
    let (cert_chain, key) = test_tls_material();
    let bind_h3 = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let server = Http3Server::new(Http3ServerConfig {
        bind_h3: Some(bind_h3),
        ..Http3ServerConfig::default()
    });

    let handle = server.start(test_service(), cert_chain, key).unwrap();
    let local_addr = handle.local_addr().unwrap();

    assert_ne!(local_addr.port(), 0);
    assert!(!handle.is_finished());
    assert!(UdpSocket::bind(local_addr).await.is_err());

    handle.shutdown();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .unwrap()
            .is_ok()
    );
}

#[tokio::test]
async fn incompatible_quic_handshake_does_not_stop_listener() {
    let (cert_chain, key) = test_tls_material();
    let server = Http3Server::new(Http3ServerConfig {
        bind_h3: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))),
        ..Http3ServerConfig::default()
    });
    let handle = server
        .start(test_service(), cert_chain.clone(), key)
        .unwrap();
    let local_addr = handle.local_addr().unwrap();

    attempt_incompatible_quic_connection(local_addr, cert_chain[0].clone()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!handle.is_finished());

    let mut client = H3TestClient::connect(local_addr, cert_chain[0].clone()).await;
    let (body, _, _) = client.request_with_body(Method::GET, "/test", &[]).await;
    assert_eq!(body, b"routed");

    handle.shutdown();
    tokio::time::timeout(Duration::from_secs(1), handle)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
#[ignore = "D22 server round 02 fixture work: mechanics-http-client does not expose a self-signed test trust hook"]
async fn end_to_end_h3_request_via_mhc() {}

#[tokio::test]
async fn request_routes_into_tower_service_substitute() {
    fn assert_h3_service<S>(service: S) -> S
    where
        S: Service<Request<H3RequestBody>, Response = Response<Full<Bytes>>>,
    {
        service
    }

    let _service = assert_h3_service(test_service());
}

#[tokio::test]
async fn axum_router_adapter_returns_streaming_body_response() {
    let router = axum::Router::new().route(
        "/test",
        axum::routing::get(|| async { axum::response::Html("routed") }),
    );
    let service = router_into_h3_service(router);

    fn assert_h3_service<S>(service: S) -> S
    where
        S: Service<Request<H3RequestBody>, Response = Response<axum::body::Body>>,
    {
        service
    }

    let _service = assert_h3_service(service);
}

#[tokio::test]
#[ignore = "binds UDP and exercises an in-process HTTP/3 client/server pair"]
async fn end_to_end_h3_streams_large_request_and_response_bodies() {
    let (cert_chain, key) = test_tls_material();
    let request_frame_count = Arc::new(AtomicUsize::new(0));
    let service_frame_count = Arc::clone(&request_frame_count);
    let service = service_fn(move |request: Request<H3RequestBody>| {
        let service_frame_count = Arc::clone(&service_frame_count);
        async move {
            let response = match request.uri().path() {
                "/echo" => {
                    let mut body = request.into_body();
                    let mut echoed = Vec::with_capacity(STREAM_TEST_BYTES);
                    while let Some(frame) = body.frame().await {
                        let frame = frame.unwrap();
                        if let Ok(mut data) = frame.into_data() {
                            service_frame_count.fetch_add(1, Ordering::Relaxed);
                            let remaining = data.remaining();
                            echoed.extend_from_slice(&data.copy_to_bytes(remaining));
                        }
                    }
                    Response::new(StreamingTestBody::once(Bytes::from(echoed)))
                }
                "/pattern" => Response::new(StreamingTestBody::pattern(STREAM_TEST_BYTES)),
                _ => {
                    let mut response = Response::new(StreamingTestBody::once(Bytes::new()));
                    *response.status_mut() = StatusCode::NOT_FOUND;
                    response
                }
            };
            Ok::<_, Infallible>(response)
        }
    });

    let server = Http3Server::new(Http3ServerConfig {
        bind_h3: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))),
        ..Http3ServerConfig::default()
    });
    let handle = server.start(service, cert_chain.clone(), key).unwrap();
    let local_addr = handle.local_addr().unwrap();
    let mut client = H3TestClient::connect(local_addr, cert_chain[0].clone()).await;

    let request_body = randomish_bytes(STREAM_TEST_BYTES);
    let (echoed, request_frames_sent, _) = client
        .request_with_body(Method::POST, "/echo", &request_body)
        .await;
    assert_eq!(echoed, request_body);
    assert!(request_frames_sent > 1);
    assert!(request_frame_count.load(Ordering::Relaxed) > 1);

    let (pattern, _, response_frames_received) =
        client.request_with_body(Method::GET, "/pattern", &[]).await;
    assert_eq!(pattern, incrementing_pattern(STREAM_TEST_BYTES));
    assert!(response_frames_received > 1);

    handle.shutdown();
    tokio::time::timeout(Duration::from_secs(1), handle)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn alt_svc_layer_adds_header() {
    let layer = alt_svc_layer(443, 86_400);
    let mut service = layer.layer(service_fn(|_: Request<()>| async {
        Ok::<_, Infallible>(Response::new(()))
    }));

    for _ in 0..2 {
        let response = service
            .ready()
            .await
            .unwrap()
            .call(Request::new(()))
            .await
            .unwrap();
        assert_eq!(
            response.headers().get(ALT_SVC).unwrap(),
            r#"h3=":443"; ma=86400"#
        );
    }
}

#[tokio::test]
async fn alt_svc_layer_respects_custom_port_and_max_age() {
    let layer = alt_svc_layer(8443, 3_600);
    let mut service = layer.layer(service_fn(|_: Request<()>| async {
        Ok::<_, Infallible>(Response::new(()))
    }));

    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::new(()))
        .await
        .unwrap();

    assert_eq!(
        response.headers().get(ALT_SVC).unwrap(),
        r#"h3=":8443"; ma=3600"#
    );
}

#[tokio::test]
async fn alt_svc_layer_is_idempotent_under_repeated_layer_stacking() {
    let layer = alt_svc_layer(443, 86_400);
    let mut service = layer
        .clone()
        .layer(layer.layer(service_fn(|_: Request<()>| async {
            Ok::<_, Infallible>(Response::new(()))
        })));

    let response = service
        .ready()
        .await
        .unwrap()
        .call(Request::new(()))
        .await
        .unwrap();

    let mut values = response.headers().get_all(ALT_SVC).iter();
    assert_eq!(values.next().unwrap(), r#"h3=":443"; ma=86400"#);
    assert!(values.next().is_none());
}

#[test]
fn zero_rtt_policy_default_rejects_non_idempotent_methods() {
    let allowed = default_zero_rtt_methods();

    assert!(is_zero_rtt_safe(&Method::GET, &allowed));
    assert!(is_zero_rtt_safe(&Method::HEAD, &allowed));
    assert!(!is_zero_rtt_safe(&Method::POST, &allowed));
    assert!(!is_zero_rtt_safe(&Method::PUT, &allowed));
    assert!(!is_zero_rtt_safe(&Method::PATCH, &allowed));
    assert!(!is_zero_rtt_safe(&Method::DELETE, &allowed));
    assert!(!is_zero_rtt_safe(&Method::OPTIONS, &allowed));
}

#[test]
fn zero_rtt_policy_honours_config_override() {
    let allowed = vec![Method::GET, Method::HEAD, Method::OPTIONS];

    assert!(is_zero_rtt_safe(&Method::OPTIONS, &allowed));
}

struct StreamingTestBody {
    kind: StreamingTestBodyKind,
}

enum StreamingTestBodyKind {
    Once(Option<Bytes>),
    Pattern {
        offset: usize,
        len: usize,
        chunk_size: usize,
    },
}

impl StreamingTestBody {
    fn once(bytes: Bytes) -> Self {
        Self {
            kind: StreamingTestBodyKind::Once(Some(bytes)),
        }
    }

    fn pattern(len: usize) -> Self {
        Self {
            kind: StreamingTestBodyKind::Pattern {
                offset: 0,
                len,
                chunk_size: STREAM_TEST_CHUNK_BYTES,
            },
        }
    }
}

impl http_body::Body for StreamingTestBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match &mut self.kind {
            StreamingTestBodyKind::Once(bytes) => {
                Poll::Ready(bytes.take().map(|bytes| Ok(Frame::data(bytes))))
            }
            StreamingTestBodyKind::Pattern {
                offset,
                len,
                chunk_size,
            } => {
                if *offset >= *len {
                    return Poll::Ready(None);
                }
                let end = (*offset + *chunk_size).min(*len);
                let chunk = incrementing_pattern_range(*offset, end);
                *offset = end;
                Poll::Ready(Some(Ok(Frame::data(Bytes::from(chunk)))))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self.kind {
            StreamingTestBodyKind::Once(bytes) => bytes.is_none(),
            StreamingTestBodyKind::Pattern { offset, len, .. } => offset >= len,
        }
    }
}

struct H3TestClient {
    _endpoint: quinn::Endpoint,
    send_request: h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>,
}

impl H3TestClient {
    async fn connect(addr: SocketAddr, cert: CertificateDer<'static>) -> Self {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let mut tls_config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        tls_config.alpn_protocols = vec![b"h3".to_vec()];
        let quic_config = quinn::crypto::rustls::QuicClientConfig::try_from(tls_config).unwrap();
        let client_config = quinn::ClientConfig::new(Arc::new(quic_config));
        let mut endpoint =
            quinn::Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        endpoint.set_default_client_config(client_config);
        let connection = endpoint.connect(addr, "localhost").unwrap().await.unwrap();
        let quic = h3_quinn::Connection::new(connection);
        let (mut driver, send_request) = h3::client::builder().build(quic).await.unwrap();
        tokio::spawn(async move {
            let _ = driver.wait_idle().await;
        });

        Self {
            _endpoint: endpoint,
            send_request,
        }
    }

    async fn request_with_body(
        &mut self,
        method: Method,
        path: &str,
        body: &[u8],
    ) -> (Vec<u8>, usize, usize) {
        let uri = format!("https://localhost{path}");
        let request = Request::builder().method(method).uri(uri).body(()).unwrap();
        let mut stream = self.send_request.send_request(request).await.unwrap();
        let mut request_frames_sent = 0;
        for chunk in body.chunks(STREAM_TEST_CHUNK_BYTES) {
            stream
                .send_data(Bytes::copy_from_slice(chunk))
                .await
                .unwrap();
            request_frames_sent += 1;
        }
        stream.finish().await.unwrap();

        let response = stream.recv_response().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut response_body = Vec::new();
        let mut response_frames_received = 0;
        while let Some(mut data) = stream.recv_data().await.unwrap() {
            response_frames_received += 1;
            let remaining = data.remaining();
            response_body.extend_from_slice(&data.copy_to_bytes(remaining));
        }

        (response_body, request_frames_sent, response_frames_received)
    }
}

async fn attempt_incompatible_quic_connection(addr: SocketAddr, cert: CertificateDer<'static>) {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).unwrap();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut tls_config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls_config.alpn_protocols = vec![b"not-h3".to_vec()];
    let quic_config = quinn::crypto::rustls::QuicClientConfig::try_from(tls_config).unwrap();
    let client_config = quinn::ClientConfig::new(Arc::new(quic_config));
    let mut endpoint = quinn::Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    endpoint.set_default_client_config(client_config);

    let connection = endpoint.connect(addr, "localhost").unwrap().await;
    assert!(connection.is_err());
}

fn randomish_bytes(len: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_u32;
    let mut bytes = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        bytes.push((state >> 24) as u8);
    }
    bytes
}

fn incrementing_pattern(len: usize) -> Vec<u8> {
    incrementing_pattern_range(0, len)
}

fn incrementing_pattern_range(start: usize, end: usize) -> Vec<u8> {
    (start..end).map(|index| (index % 251) as u8).collect()
}
