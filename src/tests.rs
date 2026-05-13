use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use bytes::Bytes;
use http::header::ALT_SVC;
use http::{Method, Request, Response, StatusCode};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::UdpSocket;
use tower::util::BoxCloneService;
use tower::{Layer, Service, ServiceExt, service_fn};

use crate::{
    Http3Server, Http3ServerConfig, alt_svc_layer, default_zero_rtt_methods, is_zero_rtt_safe,
};

fn test_service() -> BoxCloneService<Request<()>, Response<Bytes>, Infallible> {
    service_fn(|request: Request<()>| async move {
        let body = if request.uri().path() == "/test" {
            Bytes::from_static(b"routed")
        } else {
            Bytes::new()
        };
        Ok::<_, Infallible>(Response::new(body))
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
#[ignore = "D22 server round 02 fixture work: mechanics-http-client does not expose a self-signed test trust hook"]
async fn end_to_end_h3_request_via_mhc() {}

#[tokio::test]
async fn request_routes_into_tower_service_substitute() {
    let mut service = test_service();
    let request = Request::builder().uri("/test").body(()).unwrap();

    let response = service.ready().await.unwrap().call(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.into_body(), Bytes::from_static(b"routed"));
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
