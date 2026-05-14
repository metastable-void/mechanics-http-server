//! Compatibility helpers for routing HTTP/3 requests into axum.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response};
use tower::{Service, ServiceExt};

use crate::H3RequestBody;

/// Convert an axum [`Router`] into the tower service shape accepted
/// by [`crate::Http3Server::start`].
///
/// The adapter wraps [`H3RequestBody`] in axum's [`Body`] and returns
/// axum's response body directly, so both request and response bodies
/// stream through the HTTP/3 server without an intermediate buffer cap.
pub fn router_into_h3_service(router: Router) -> AxumH3Service {
    AxumH3Service { router }
}

/// Service returned by [`router_into_h3_service`].
#[derive(Clone, Debug)]
pub struct AxumH3Service {
    router: Router,
}

impl Service<Request<H3RequestBody>> for AxumH3Service {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<H3RequestBody>) -> Self::Future {
        let router = self.router.clone();
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let request = Request::from_parts(parts, Body::new(body));
            router.oneshot(request).await.map_err(match_infallible)
        })
    }
}

fn match_infallible(error: Infallible) -> Infallible {
    match error {}
}
