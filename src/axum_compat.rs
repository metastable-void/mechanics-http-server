//! Compatibility helpers for routing HTTP/3 requests into axum.

use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, Response};
use bytes::Bytes;
use tower::{Service, ServiceExt};

/// Default cap for buffering an axum response body for HTTP/3.
pub const DEFAULT_H3_RESPONSE_BODY_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Convert an axum [`Router`] into the tower service shape accepted
/// by [`crate::Http3Server::start`].
///
/// The current server API exposes HTTP/3 request bodies to services as
/// `Request<()>` and accepts a single `Bytes` response body chunk. This
/// adapter therefore forwards an empty axum request body and buffers
/// the axum response body up to
/// [`DEFAULT_H3_RESPONSE_BODY_LIMIT_BYTES`].
pub fn router_into_h3_service(router: Router) -> AxumH3Service {
    router_into_h3_service_with_response_limit(router, DEFAULT_H3_RESPONSE_BODY_LIMIT_BYTES)
}

/// Convert an axum [`Router`] into an HTTP/3 service with a custom
/// buffered response-body cap.
pub fn router_into_h3_service_with_response_limit(
    router: Router,
    response_body_limit_bytes: usize,
) -> AxumH3Service {
    AxumH3Service {
        router,
        response_body_limit_bytes,
    }
}

/// Service returned by [`router_into_h3_service`].
#[derive(Clone, Debug)]
pub struct AxumH3Service {
    router: Router,
    response_body_limit_bytes: usize,
}

impl Service<Request<()>> for AxumH3Service {
    type Response = Response<Bytes>;
    type Error = AxumCompatError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<()>) -> Self::Future {
        let router = self.router.clone();
        let response_body_limit_bytes = self.response_body_limit_bytes;
        Box::pin(async move {
            let (parts, ()) = request.into_parts();
            let request = Request::from_parts(parts, Body::empty());
            let response = router.oneshot(request).await.map_err(match_infallible)?;
            let (parts, body) = response.into_parts();
            let body = to_bytes(body, response_body_limit_bytes)
                .await
                .map_err(AxumCompatError::Body)?;
            Ok(Response::from_parts(parts, body))
        })
    }
}

/// Error returned by [`AxumH3Service`].
#[derive(Debug)]
pub enum AxumCompatError {
    /// Axum response-body collection failed or exceeded the cap.
    Body(axum::Error),
}

impl fmt::Display for AxumCompatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Body(error) => write!(f, "axum response body failed: {error}"),
        }
    }
}

impl StdError for AxumCompatError {}

fn match_infallible(error: Infallible) -> AxumCompatError {
    match error {}
}
