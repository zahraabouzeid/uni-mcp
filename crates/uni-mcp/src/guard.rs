//! Protection for a server on the internet: limits per client address, a
//! same-origin check for the dashboard's API, and security headers.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{HOST, ORIGIN, RETRY_AFTER};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::accounts;

const MAX_TRACKED: usize = 10_000;

const HEADERS: [(&str, &str); 4] = [
    ("x-frame-options", "DENY"),
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "no-referrer"),
    ("content-security-policy", "frame-ancestors 'none'"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Login,
    Registration,
    Token,
}

impl Kind {
    /// Attempts allowed per window, and the window in seconds.
    fn limit(self) -> (u32, u64) {
        match self {
            Self::Login => (10, 15 * 60),
            Self::Registration => (20, 60 * 60),
            Self::Token => (60, 60),
        }
    }
}

pub struct Guard {
    /// Behind a proxy every request comes from the proxy, which names the
    /// client in `X-Forwarded-For`; without one, that header could be forged.
    behind_proxy: bool,
    attempts: Mutex<HashMap<(Kind, IpAddr), (u32, u64)>>,
}

impl Guard {
    pub fn new(behind_proxy: bool) -> Arc<Self> {
        Arc::new(Self {
            behind_proxy,
            attempts: Mutex::default(),
        })
    }

    /// Counts an attempt, or says how many seconds remain until the next one.
    fn attempt(&self, kind: Kind, client: IpAddr, now: u64) -> Result<(), u64> {
        let mut attempts = self.attempts.lock().unwrap_or_else(PoisonError::into_inner);
        if attempts.len() >= MAX_TRACKED {
            attempts.retain(|(kind, _), (_, since)| now < *since + kind.limit().1);
        }
        let (limit, window) = kind.limit();
        let (count, since) = attempts.entry((kind, client)).or_insert((0, now));
        if now >= *since + window {
            (*count, *since) = (0, now);
        }
        if *count >= limit {
            return Err(*since + window - now);
        }
        *count += 1;
        Ok(())
    }

    fn client(&self, request: &Request) -> IpAddr {
        let forwarded = || {
            let header = request.headers().get("x-forwarded-for")?.to_str().ok()?;
            header.rsplit(',').next()?.trim().parse().ok()
        };
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        self.behind_proxy
            .then(forwarded)
            .flatten()
            .or(peer)
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
    }
}

pub async fn limit(
    State((guard, kind)): State<(Arc<Guard>, Kind)>,
    request: Request,
    next: Next,
) -> Response {
    let client = guard.client(&request);
    match guard.attempt(kind, client, accounts::now()) {
        Ok(()) => next.run(request).await,
        Err(seconds) => {
            let minutes = seconds.div_ceil(60).max(1);
            let unit = if minutes == 1 { "minute" } else { "minutes" };
            let body =
                json!({ "error": format!("too many attempts; try again in {minutes} {unit}") });
            let wait = [(RETRY_AFTER, seconds.to_string())];
            (StatusCode::TOO_MANY_REQUESTS, wait, Json(body)).into_response()
        }
    }
}

pub async fn same_origin(request: Request, next: Next) -> Response {
    let reads = matches!(*request.method(), Method::GET | Method::HEAD);
    if !reads && !from_this_site(request.headers()) {
        let body = json!({ "error": "the request came from another site" });
        return (StatusCode::FORBIDDEN, Json(body)).into_response();
    }
    next.run(request).await
}

fn from_this_site(headers: &HeaderMap) -> bool {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    match header(ORIGIN.as_str()) {
        Some(origin) => origin.split_once("://").map(|(_, host)| host) == header(HOST.as_str()),
        None => header("sec-fetch-site").is_none_or(|site| site != "cross-site"),
    }
}

pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (name, value) in HEADERS {
        response.headers_mut().insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::middleware;
    use axum::routing::post;
    use tower::ServiceExt;

    use super::*;

    const HOME: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
    const OTHER: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2));

    #[test]
    fn each_address_has_its_own_attempts_and_they_come_back_after_the_window() {
        let guard = Guard::new(false);
        for _ in 0..10 {
            assert!(guard.attempt(Kind::Login, HOME, 0).is_ok());
        }
        assert_eq!(guard.attempt(Kind::Login, HOME, 60), Err(15 * 60 - 60));
        assert!(guard.attempt(Kind::Login, OTHER, 60).is_ok());
        assert!(guard.attempt(Kind::Token, HOME, 60).is_ok());
        assert!(guard.attempt(Kind::Login, HOME, 15 * 60).is_ok());
    }

    fn request(forwarded: Option<&str>) -> Request {
        let mut request = Request::new(Body::empty());
        if let Some(forwarded) = forwarded {
            request
                .headers_mut()
                .insert("x-forwarded-for", forwarded.parse().expect("header"));
        }
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((HOME, 1234))));
        request
    }

    #[test]
    fn the_forwarded_address_counts_only_behind_a_proxy() {
        let forged = Some("203.0.113.9, 192.0.2.2");
        assert_eq!(Guard::new(false).client(&request(forged)), HOME);
        assert_eq!(Guard::new(true).client(&request(forged)), OTHER);
        assert_eq!(Guard::new(true).client(&request(None)), HOME);
    }

    #[test]
    fn only_requests_from_this_site_or_without_a_site_pass() {
        let headers = |pairs: &[(&'static str, &str)]| {
            let mut headers = HeaderMap::new();
            for (name, value) in pairs {
                headers.insert(*name, value.parse().expect("header"));
            }
            headers
        };
        let host = ("host", "uni.example.org");
        assert!(from_this_site(&headers(&[
            host,
            ("origin", "https://uni.example.org")
        ])));
        assert!(!from_this_site(&headers(&[
            host,
            ("origin", "https://evil.example")
        ])));
        assert!(!from_this_site(&headers(&[host, ("origin", "null")])));
        assert!(!from_this_site(&headers(&[
            host,
            ("sec-fetch-site", "cross-site")
        ])));
        assert!(from_this_site(&headers(&[
            host,
            ("sec-fetch-site", "same-origin")
        ])));
        assert!(from_this_site(&headers(&[host])));
    }

    #[tokio::test]
    async fn a_limited_route_answers_429_with_the_wait_and_every_answer_has_the_headers() {
        let guard = Guard::new(false);
        let router = Router::new()
            .route("/login", post(|| async { "ok" }))
            .layer(middleware::from_fn_with_state((guard, Kind::Login), limit))
            .layer(middleware::from_fn(security_headers));
        let send = || {
            router.clone().oneshot(
                Request::post("/login")
                    .body(Body::empty())
                    .expect("request"),
            )
        };
        for _ in 0..10 {
            assert_eq!(send().await.expect("response").status(), StatusCode::OK);
        }
        let refused = send().await.expect("response");
        assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(refused.headers().contains_key(RETRY_AFTER));
        assert_eq!(refused.headers()["x-frame-options"], "DENY");
    }
}
