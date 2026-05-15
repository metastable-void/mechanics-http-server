//! HTTP/3 request body adapter.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes};
use h3::server::RequestStream;
use http_body::Frame;

type H3RecvStream = RequestStream<h3_quinn::RecvStream, Bytes>;
type RecvDataFuture =
    Pin<Box<dyn Future<Output = (H3RecvStream, Result<Option<Bytes>, H3RequestBodyError>)> + Send>>;

/// Streaming request body received from an HTTP/3 client.
///
/// `H3RequestBody` is the request-body type passed to services by
/// [`crate::Http3Server::start`]. It implements [`http_body::Body`]
/// and yields HTTP/3 DATA frames as [`Bytes`].
pub struct H3RequestBody {
    stream: Option<H3RecvStream>,
    state: H3RequestBodyState,
}

enum H3RequestBodyState {
    ReadingData,
    DataFuture(RecvDataFuture),
    Done,
}

impl H3RequestBody {
    pub(crate) fn new(stream: H3RecvStream) -> Self {
        Self {
            stream: Some(stream),
            state: H3RequestBodyState::ReadingData,
        }
    }
}

impl std::fmt::Debug for H3RequestBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H3RequestBody")
            .field(
                "is_end_stream",
                &matches!(self.state, H3RequestBodyState::Done),
            )
            .finish_non_exhaustive()
    }
}

/// Error returned while receiving an HTTP/3 request body.
#[derive(Debug, thiserror::Error)]
pub enum H3RequestBodyError {
    /// The underlying HTTP/3 stream returned an error while receiving data.
    #[error("HTTP/3 request body failed: {0}")]
    Stream(String),
}

impl H3RequestBodyError {
    fn stream(error: h3::error::StreamError) -> Self {
        Self::Stream(error.to_string())
    }
}

impl http_body::Body for H3RequestBody {
    type Data = Bytes;
    type Error = H3RequestBodyError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        loop {
            match &mut self.state {
                H3RequestBodyState::ReadingData => {
                    let Some(mut stream) = self.stream.take() else {
                        self.state = H3RequestBodyState::Done;
                        return Poll::Ready(None);
                    };
                    self.state = H3RequestBodyState::DataFuture(Box::pin(async move {
                        let result = stream.recv_data().await.map_err(H3RequestBodyError::stream);
                        let result = result.map(|chunk| {
                            chunk.map(|mut chunk| {
                                let remaining = chunk.remaining();
                                chunk.copy_to_bytes(remaining)
                            })
                        });
                        (stream, result)
                    }));
                }
                H3RequestBodyState::DataFuture(future) => {
                    let (stream, result) = match future.as_mut().poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(result) => result,
                    };
                    self.stream = Some(stream);
                    match result {
                        Ok(Some(bytes)) => {
                            self.state = H3RequestBodyState::ReadingData;
                            return Poll::Ready(Some(Ok(Frame::data(bytes))));
                        }
                        Ok(None) => {
                            self.state = H3RequestBodyState::Done;
                            return Poll::Ready(None);
                        }
                        Err(error) => {
                            self.state = H3RequestBodyState::Done;
                            return Poll::Ready(Some(Err(error)));
                        }
                    }
                }
                H3RequestBodyState::Done => return Poll::Ready(None),
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self.state, H3RequestBodyState::Done)
    }
}
