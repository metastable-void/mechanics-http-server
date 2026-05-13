//! `Alt-Svc` response middleware for advertising the QUIC listener.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::header::{ALT_SVC, HeaderValue};
use http::{Request, Response};
use tower::{Layer, Service};

/// Build an [`AltSvcLayer`] for `h3=":<port>"; ma=<max-age>`.
pub fn alt_svc_layer(h3_port: u16, max_age_secs: u64) -> AltSvcLayer {
    AltSvcLayer::new(h3_port, max_age_secs)
}

/// Tower layer that inserts an HTTP/3 `Alt-Svc` response header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AltSvcLayer {
    header_value: HeaderValue,
}

impl AltSvcLayer {
    /// Build an `AltSvcLayer` for `h3=":<port>"; ma=<max-age>`.
    pub fn new(h3_port: u16, max_age_secs: u64) -> Self {
        let value = format!("h3=\":{h3_port}\"; ma={max_age_secs}");
        Self {
            header_value: HeaderValue::from_str(&value)
                .unwrap_or_else(|_| HeaderValue::from_static("h3=\":443\"; ma=86400")),
        }
    }
}

impl<S> Layer<S> for AltSvcLayer {
    type Service = AltSvcService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AltSvcService {
            inner,
            header_value: self.header_value.clone(),
        }
    }
}

/// Service produced by [`AltSvcLayer`].
#[derive(Clone, Debug)]
pub struct AltSvcService<S> {
    inner: S,
    header_value: HeaderValue,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for AltSvcService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    S::Future: Send + 'static,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let future = self.inner.call(req);
        let header_value = self.header_value.clone();
        Box::pin(async move {
            let mut response = future.await?;
            response.headers_mut().insert(ALT_SVC, header_value);
            Ok(response)
        })
    }
}
