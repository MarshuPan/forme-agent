//! Exact compiled asset table. Request paths are never opened on the filesystem.
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

#[path = "../../../assets/product/embedded.rs"]
mod embedded;

pub(super) async fn index() -> Response {
    respond("/index.html")
}
pub(super) async fn file(uri: Uri) -> Response {
    respond(uri.path())
}

fn respond(path: &str) -> Response {
    let Some((mime, body)) = embedded::asset(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = body.into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
