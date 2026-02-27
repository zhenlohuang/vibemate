use std::convert::Infallible;
use std::io;

use axum::response::{IntoResponse, Response, Sse};
use axum::response::sse::Event;
use futures::StreamExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

pub fn relay_sse_stream(upstream_response: reqwest::Response) -> Response {
    let stream = upstream_response
        .bytes_stream()
        .map(|chunk| chunk.map_err(|err| io::Error::new(io::ErrorKind::Other, err)));

    let reader = StreamReader::new(stream);
    let mut lines = BufReader::new(reader).lines();

    let sse_stream = async_stream::stream! {
        while let Ok(Some(line)) = lines.next_line().await {
            if line.is_empty() {
                continue;
            }

            let data = line
                .strip_prefix("data:")
                .map(str::trim_start)
                .unwrap_or(&line)
                .to_string();

            yield Ok::<Event, Infallible>(Event::default().data(data));
        }
    };

    Sse::new(sse_stream).into_response()
}
