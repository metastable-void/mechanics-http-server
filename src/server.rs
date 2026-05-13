//! HTTP/3 QUIC server lifecycle.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use quinn::crypto::rustls::QuicServerConfig;
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};
use tower::{Service, ServiceExt};

use crate::error::{Error, Result};
use crate::zero_rtt::{default_zero_rtt_methods, is_zero_rtt_safe};

/// Optional HTTP/3 listener that runs alongside an existing TCP+TLS server.
#[derive(Clone, Debug)]
pub struct Http3Server {
    config: Http3ServerConfig,
}

/// Configuration for the HTTP/3 listener.
#[derive(Clone, Debug)]
pub struct Http3ServerConfig {
    /// UDP bind address. `None` makes [`Http3Server::start`] fully inert.
    pub bind_h3: Option<SocketAddr>,
    /// Default `Alt-Svc` max-age value, in seconds.
    pub alt_svc_max_age_secs: u64,
    /// Methods allowed by the 0-RTT replay-safety policy.
    pub zero_rtt_idempotent_methods: Vec<http::Method>,
}

impl Default for Http3ServerConfig {
    fn default() -> Self {
        Self {
            bind_h3: None,
            alt_svc_max_age_secs: 86_400,
            zero_rtt_idempotent_methods: default_zero_rtt_methods(),
        }
    }
}

impl Http3Server {
    /// Build a server value without starting it.
    pub fn new(config: Http3ServerConfig) -> Self {
        Self { config }
    }

    /// Start the HTTP/3 listener and route requests into `service`.
    ///
    /// The service receives HTTP/3 request headers as [`Request<()>`]
    /// and returns response headers plus a single [`Bytes`] response
    /// body chunk.
    pub fn start<S>(
        self,
        service: S,
        tls_cert_chain: Vec<rustls::pki_types::CertificateDer<'static>>,
        tls_private_key: rustls::pki_types::PrivateKeyDer<'static>,
    ) -> Result<Http3Handle>
    where
        S: Service<Request<()>, Response = Response<Bytes>> + Clone + Send + 'static,
        S::Future: Send + 'static,
        S::Error: std::fmt::Display + Send + Sync + 'static,
    {
        let Some(bind_addr) = self.config.bind_h3 else {
            return Ok(Http3Handle::inert());
        };

        let server_config = quic_server_config(tls_cert_chain, tls_private_key)?;
        let endpoint = quinn::Endpoint::server(server_config, bind_addr)
            .map_err(|e| Error::BindFailed(e.to_string()))?;
        let local_addr = endpoint
            .local_addr()
            .map_err(|e| Error::BindFailed(e.to_string()))?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let allowed_zero_rtt_methods = Arc::new(self.config.zero_rtt_idempotent_methods);
        let task_endpoint = endpoint.clone();
        let accept_task = tokio::spawn(async move {
            accept_loop(
                task_endpoint,
                shutdown_rx,
                service,
                allowed_zero_rtt_methods,
            )
            .await
        });

        Ok(Http3Handle::running(
            endpoint,
            local_addr,
            shutdown_tx,
            accept_task,
        ))
    }
}

/// Handle returned by [`Http3Server::start`].
///
/// Awaiting the handle waits for the accept loop to finish. Dropping
/// it closes the QUIC endpoint and signals the accept loop to stop
/// accepting new connections while existing connection tasks drain.
pub struct Http3Handle {
    state: Http3HandleState,
}

enum Http3HandleState {
    Inert,
    Running {
        endpoint: quinn::Endpoint,
        local_addr: SocketAddr,
        shutdown_tx: watch::Sender<bool>,
        accept_task: JoinHandle<Result<()>>,
    },
    Done,
}

impl Http3Handle {
    fn inert() -> Self {
        Self {
            state: Http3HandleState::Inert,
        }
    }

    fn running(
        endpoint: quinn::Endpoint,
        local_addr: SocketAddr,
        shutdown_tx: watch::Sender<bool>,
        accept_task: JoinHandle<Result<()>>,
    ) -> Self {
        Self {
            state: Http3HandleState::Running {
                endpoint,
                local_addr,
                shutdown_tx,
                accept_task,
            },
        }
    }

    /// Return whether this handle is the no-op `bind_h3 = None` variant.
    pub fn is_inert(&self) -> bool {
        matches!(self.state, Http3HandleState::Inert | Http3HandleState::Done)
    }

    /// Return the bound UDP address for a running HTTP/3 listener.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        match &self.state {
            Http3HandleState::Running { local_addr, .. } => Some(*local_addr),
            Http3HandleState::Inert | Http3HandleState::Done => None,
        }
    }

    /// Return whether the accept-loop task has finished.
    pub fn is_finished(&self) -> bool {
        match &self.state {
            Http3HandleState::Inert | Http3HandleState::Done => true,
            Http3HandleState::Running { accept_task, .. } => accept_task.is_finished(),
        }
    }

    /// Signal shutdown without consuming the handle.
    pub fn shutdown(&self) {
        if let Http3HandleState::Running {
            endpoint,
            shutdown_tx,
            ..
        } = &self.state
        {
            let _ = shutdown_tx.send(true);
            endpoint.close(0_u32.into(), b"shutdown");
        }
    }
}

impl Future for Http3Handle {
    type Output = Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match &mut this.state {
            Http3HandleState::Inert | Http3HandleState::Done => Poll::Ready(Ok(())),
            Http3HandleState::Running { accept_task, .. } => match Pin::new(accept_task).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(joined) => {
                    this.state = Http3HandleState::Done;
                    Poll::Ready(
                        joined.map_err(|e| Error::Internal(format!("accept task failed: {e}")))?,
                    )
                }
            },
        }
    }
}

impl Drop for Http3Handle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn quic_server_config(
    tls_cert_chain: Vec<rustls::pki_types::CertificateDer<'static>>,
    tls_private_key: rustls::pki_types::PrivateKeyDer<'static>,
) -> Result<quinn::ServerConfig> {
    if tls_cert_chain.is_empty() {
        return Err(Error::InvalidTls("empty certificate chain".to_owned()));
    }

    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut tls_config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::InvalidTls(e.to_string()))?
        .with_no_client_auth()
        .with_single_cert(tls_cert_chain, tls_private_key)
        .map_err(|e| Error::InvalidTls(e.to_string()))?;
    tls_config.alpn_protocols = vec![b"h3".to_vec()];
    tls_config.max_early_data_size = 0;

    let quic_config =
        QuicServerConfig::try_from(tls_config).map_err(|e| Error::InvalidTls(e.to_string()))?;
    Ok(quinn::ServerConfig::with_crypto(Arc::new(quic_config)))
}

async fn accept_loop<S>(
    endpoint: quinn::Endpoint,
    mut shutdown_rx: watch::Receiver<bool>,
    service: S,
    allowed_zero_rtt_methods: Arc<Vec<http::Method>>,
) -> Result<()>
where
    S: Service<Request<()>, Response = Response<Bytes>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Display + Send + Sync + 'static,
{
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_ok() && *shutdown_rx.borrow() {
                    break;
                }
                if changed.is_err() {
                    break;
                }
            }
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else {
                    break;
                };
                let connecting = incoming
                    .accept()
                    .map_err(|e| Error::Internal(format!("incoming connection rejected: {e}")))?;
                let connection_service = service.clone();
                let connection_allowed = allowed_zero_rtt_methods.clone();
                connections.spawn(async move {
                    handle_connection(connecting, connection_service, connection_allowed).await
                });
            }
            joined = connections.join_next(), if !connections.is_empty() => {
                match joined {
                    Some(Ok(Ok(()))) | None => {}
                    Some(Ok(Err(e))) => tracing::warn!("HTTP/3 connection failed: {e}"),
                    Some(Err(e)) => tracing::warn!("HTTP/3 connection task failed: {e}"),
                }
            }
        }
    }

    while let Some(joined) = connections.join_next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!("HTTP/3 connection failed during shutdown: {e}"),
            Err(e) => tracing::warn!("HTTP/3 connection task failed during shutdown: {e}"),
        }
    }

    Ok(())
}

async fn handle_connection<S>(
    connecting: quinn::Connecting,
    service: S,
    allowed_zero_rtt_methods: Arc<Vec<http::Method>>,
) -> Result<()>
where
    S: Service<Request<()>, Response = Response<Bytes>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Display + Send + Sync + 'static,
{
    let connection = connecting
        .await
        .map_err(|e| Error::Internal(format!("QUIC handshake failed: {e}")))?;
    let quic = h3_quinn::Connection::new(connection);
    let mut h3_conn = h3::server::Connection::new(quic)
        .await
        .map_err(|e| Error::Internal(format!("HTTP/3 connection failed: {e}")))?;
    let mut streams = JoinSet::new();

    loop {
        match h3_conn.accept().await {
            Ok(Some(resolver)) => {
                let stream_service = service.clone();
                let stream_allowed = allowed_zero_rtt_methods.clone();
                streams.spawn(async move {
                    if let Err(e) =
                        handle_request(resolver, stream_service, stream_allowed.as_slice()).await
                    {
                        tracing::warn!("HTTP/3 request failed: {e}");
                    }
                });
            }
            Ok(None) => break,
            Err(e) => return Err(Error::Internal(format!("HTTP/3 accept failed: {e}"))),
        }

        while let Some(joined) = streams.try_join_next() {
            if let Err(e) = joined {
                tracing::warn!("HTTP/3 request task failed: {e}");
            }
        }
    }

    while let Some(joined) = streams.join_next().await {
        if let Err(e) = joined {
            tracing::warn!("HTTP/3 request task failed during drain: {e}");
        }
    }

    Ok(())
}

async fn handle_request<S>(
    resolver: h3::server::RequestResolver<h3_quinn::Connection, Bytes>,
    mut service: S,
    allowed_zero_rtt_methods: &[http::Method],
) -> Result<()>
where
    S: Service<Request<()>, Response = Response<Bytes>> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: std::fmt::Display + Send + Sync + 'static,
{
    let (request, mut stream) = resolver
        .resolve_request()
        .await
        .map_err(|e| Error::Internal(format!("HTTP/3 request resolution failed: {e}")))?;

    if !is_zero_rtt_safe(request.method(), allowed_zero_rtt_methods) {
        tracing::trace!(
            method = %request.method(),
            "HTTP/3 request method is outside the configured 0-RTT allow-list"
        );
    }

    let response = match service.ready().await {
        Ok(ready) => ready
            .call(request)
            .await
            .map_err(|e| Error::Internal(format!("HTTP/3 service failed: {e}")))?,
        Err(e) => {
            tracing::warn!("HTTP/3 service was not ready: {e}");
            Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Bytes::new())
                .map_err(|e| Error::Internal(format!("service-unavailable response failed: {e}")))?
        }
    };

    let (parts, body) = response.into_parts();
    stream
        .send_response(Response::from_parts(parts, ()))
        .await
        .map_err(|e| Error::Internal(format!("HTTP/3 response headers failed: {e}")))?;
    if !body.is_empty() {
        stream
            .send_data(body)
            .await
            .map_err(|e| Error::Internal(format!("HTTP/3 response body failed: {e}")))?;
    }
    stream
        .finish()
        .await
        .map_err(|e| Error::Internal(format!("HTTP/3 response finish failed: {e}")))?;

    Ok(())
}
