use axum::response::Html;
use konst::{option::unwrap, slice::bytes_find, string::str_concat};

const MARKER: &[u8] = b"%VERSION%";

static INDEX_HTML: &str = {
    const INPUT: &str = include_str!("index.html");

    const MARKER_POS: usize = unwrap!(bytes_find(INPUT.as_bytes(), MARKER));

    const BEFORE: &str = unsafe { INPUT.get_unchecked(..MARKER_POS) };
    const AFTER: &str = unsafe { INPUT.get_unchecked(MARKER_POS + MARKER.len()..) };

    str_concat!(&[BEFORE, env!("CARGO_PKG_VERSION"), AFTER])
};

pub async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}
