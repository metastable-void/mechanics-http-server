//! Conservative 0-RTT replay-safety policy.

use http::Method;

/// Default methods considered safe for replayable 0-RTT requests.
pub fn default_zero_rtt_methods() -> Vec<Method> {
    vec![Method::GET, Method::HEAD]
}

/// Return whether `method` is allowed by the configured 0-RTT policy.
pub fn is_zero_rtt_safe(method: &Method, allowed: &[Method]) -> bool {
    allowed.iter().any(|candidate| candidate == method)
}
