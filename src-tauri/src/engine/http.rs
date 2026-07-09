//! The one HTTP client every transfer shares.
//!
//! Building a `reqwest::Client` per request throws away its connection pool, so
//! each segment and each retry pays a fresh TCP and TLS handshake. One process-wide
//! client lets an eight-segment download reuse keep-alive connections to the origin.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::Client;

pub const USER_AGENT: &str = "FluxDM/0.1";

/// Give up on a connection that stalls for this long. Deliberately *not*
/// `ClientBuilder::timeout`, which bounds the whole request including the body —
/// that would kill a healthy but slow multi-gigabyte segment partway through.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

static CLIENT: OnceLock<Client> = OnceLock::new();

/// The shared client. Cheap to call: `Client` is an `Arc` internally.
///
/// Callers that need a deadline on a short request (a HEAD probe, a manifest
/// fetch) should add `.timeout(..)` to that individual request.
pub fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .pool_max_idle_per_host(16)
            .build()
            .expect("HTTP client construction cannot fail with these options")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_is_shared() {
        assert!(std::ptr::eq(client(), client()));
    }
}
