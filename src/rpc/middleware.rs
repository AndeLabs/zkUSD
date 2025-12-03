//! Axum Middleware for Rate Limiting.
//!
//! Provides tower middleware layer for integrating rate limiting
//! with axum-based HTTP servers.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
};
use futures::future::BoxFuture;
use serde::Serialize;
use tower::{Layer, Service};

use super::rate_limiter::{RateLimiter, RateLimitResult};

// ═══════════════════════════════════════════════════════════════════════════════
// RATE LIMIT LAYER
// ═══════════════════════════════════════════════════════════════════════════════

/// Tower layer for rate limiting
#[derive(Clone)]
pub struct RateLimitLayer {
    limiter: Arc<RateLimiter>,
}

impl RateLimitLayer {
    /// Create new rate limit layer
    pub fn new(limiter: Arc<RateLimiter>) -> Self {
        Self { limiter }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitMiddleware {
            inner,
            limiter: self.limiter.clone(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RATE LIMIT MIDDLEWARE
// ═══════════════════════════════════════════════════════════════════════════════

/// Middleware service for rate limiting
#[derive(Clone)]
pub struct RateLimitMiddleware<S> {
    inner: S,
    limiter: Arc<RateLimiter>,
}

impl<S> Service<Request<Body>> for RateLimitMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let limiter = self.limiter.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Extract client IP
            let ip = extract_client_ip(&req);

            // Check for API key
            let api_key = extract_api_key(&req);

            // Check rate limit
            let result = if let Some(key) = api_key {
                limiter.check_api_key(&key, ip)
            } else {
                limiter.check_ip(ip)
            };

            match result {
                RateLimitResult::Allowed => {
                    // Track connection
                    limiter.connection_opened(ip);
                    let response = inner.call(req).await;
                    limiter.connection_closed(ip);
                    response
                }
                RateLimitResult::RateLimited { retry_after } => {
                    Ok(rate_limited_response(retry_after))
                }
                RateLimitResult::Blacklisted => {
                    Ok(blacklisted_response())
                }
                RateLimitResult::TooManyConnections => {
                    Ok(too_many_connections_response())
                }
            }
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Extract client IP from request
fn extract_client_ip(req: &Request<Body>) -> IpAddr {
    // Try X-Forwarded-For header first (for proxied requests)
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(value) = forwarded.to_str() {
            if let Some(first_ip) = value.split(',').next() {
                if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }

    // Try X-Real-IP
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(value) = real_ip.to_str() {
            if let Ok(ip) = value.parse::<IpAddr>() {
                return ip;
            }
        }
    }

    // Try connection info
    if let Some(connect_info) = req.extensions().get::<axum::extract::ConnectInfo<SocketAddr>>() {
        return connect_info.0.ip();
    }

    // Fallback to localhost
    "127.0.0.1".parse().unwrap()
}

/// Extract API key from request
fn extract_api_key(req: &Request<Body>) -> Option<String> {
    // Check Authorization header
    if let Some(auth) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(value) = auth.to_str() {
            if let Some(key) = value.strip_prefix("Bearer ") {
                return Some(key.to_string());
            }
            if let Some(key) = value.strip_prefix("ApiKey ") {
                return Some(key.to_string());
            }
        }
    }

    // Check X-API-Key header
    if let Some(key) = req.headers().get("x-api-key") {
        if let Ok(value) = key.to_str() {
            return Some(value.to_string());
        }
    }

    None
}

// ═══════════════════════════════════════════════════════════════════════════════
// RESPONSE BUILDERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Response for rate-limited requests
#[derive(Serialize)]
struct RateLimitedBody {
    error: &'static str,
    code: u16,
    retry_after_secs: u64,
    message: String,
}

fn rate_limited_response(retry_after: Duration) -> Response<Body> {
    let body = RateLimitedBody {
        error: "rate_limited",
        code: 429,
        retry_after_secs: retry_after.as_secs(),
        message: format!(
            "Too many requests. Please retry after {} seconds.",
            retry_after.as_secs()
        ),
    };

    let json = serde_json::to_string(&body).unwrap_or_default();

    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header(header::CONTENT_TYPE, "application/json")
        .header("Retry-After", retry_after.as_secs().to_string())
        .header("X-RateLimit-Reset", retry_after.as_secs().to_string())
        .body(Body::from(json))
        .unwrap()
}

/// Response for blacklisted IPs
#[derive(Serialize)]
struct BlacklistedBody {
    error: &'static str,
    code: u16,
    message: &'static str,
}

fn blacklisted_response() -> Response<Body> {
    let body = BlacklistedBody {
        error: "forbidden",
        code: 403,
        message: "Access denied. Your IP has been blocked.",
    };

    let json = serde_json::to_string(&body).unwrap_or_default();

    Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json))
        .unwrap()
}

/// Response for too many connections
#[derive(Serialize)]
struct TooManyConnectionsBody {
    error: &'static str,
    code: u16,
    message: &'static str,
}

fn too_many_connections_response() -> Response<Body> {
    let body = TooManyConnectionsBody {
        error: "too_many_connections",
        code: 503,
        message: "Server is at capacity. Please try again later.",
    };

    let json = serde_json::to_string(&body).unwrap_or_default();

    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header(header::CONTENT_TYPE, "application/json")
        .header("Retry-After", "30")
        .body(Body::from(json))
        .unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════════
// CLEANUP TASK
// ═══════════════════════════════════════════════════════════════════════════════

/// Spawn background cleanup task
pub async fn spawn_cleanup_task(limiter: Arc<RateLimiter>, interval_secs: u64) {
    let interval = Duration::from_secs(interval_secs);

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            limiter.cleanup();
        }
    });
}

// ═══════════════════════════════════════════════════════════════════════════════
// REQUEST ID MIDDLEWARE
// ═══════════════════════════════════════════════════════════════════════════════

/// Layer for adding request IDs
#[derive(Clone, Default)]
pub struct RequestIdLayer;

impl<S> Layer<S> for RequestIdLayer {
    type Service = RequestIdMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestIdMiddleware { inner }
    }
}

/// Middleware for adding request IDs to responses
#[derive(Clone)]
pub struct RequestIdMiddleware<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for RequestIdMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Generate request ID
            let request_id = generate_request_id();

            let response = inner.call(req).await?;

            // Add request ID to response
            let (mut parts, body) = response.into_parts();
            parts.headers.insert(
                "X-Request-ID",
                request_id.parse().unwrap_or_else(|_| "unknown".parse().unwrap()),
            );

            Ok(Response::from_parts(parts, body))
        })
    }
}

/// Generate unique request ID
fn generate_request_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let random: u32 = rand::random();

    format!("{:x}-{:08x}", timestamp, random)
}

// ═══════════════════════════════════════════════════════════════════════════════
// SECURITY HEADERS MIDDLEWARE
// ═══════════════════════════════════════════════════════════════════════════════

/// Layer for adding security headers
#[derive(Clone, Default)]
pub struct SecurityHeadersLayer;

impl<S> Layer<S> for SecurityHeadersLayer {
    type Service = SecurityHeadersMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SecurityHeadersMiddleware { inner }
    }
}

/// Middleware for security headers
#[derive(Clone)]
pub struct SecurityHeadersMiddleware<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for SecurityHeadersMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let response = inner.call(req).await?;

            let (mut parts, body) = response.into_parts();

            // Add security headers
            parts.headers.insert(
                "X-Content-Type-Options",
                "nosniff".parse().unwrap(),
            );
            parts.headers.insert(
                "X-Frame-Options",
                "DENY".parse().unwrap(),
            );
            parts.headers.insert(
                "X-XSS-Protection",
                "1; mode=block".parse().unwrap(),
            );
            parts.headers.insert(
                "Strict-Transport-Security",
                "max-age=31536000; includeSubDomains".parse().unwrap(),
            );
            parts.headers.insert(
                "Cache-Control",
                "no-store, no-cache, must-revalidate".parse().unwrap(),
            );

            Ok(Response::from_parts(parts, body))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_id_generation() {
        let id1 = generate_request_id();
        let id2 = generate_request_id();

        assert!(!id1.is_empty());
        assert_ne!(id1, id2);
    }
}
