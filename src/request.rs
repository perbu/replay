use std::net::SocketAddr;

pub struct MirrorRequest {
    pub method: http::Method,
    pub path: String,
    pub headers: http::HeaderMap,
    pub targets: Vec<SocketAddr>,
}
