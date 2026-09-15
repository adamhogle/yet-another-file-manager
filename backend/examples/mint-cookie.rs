//! Mint a signed session cookie for UI verification without an identity provider.
//!
//! This example drives the manual browser-verification workflow documented in
//! `docs/features/user-session-identity.md`: to reach authenticated states of
//! the SPA (the user menu, access filtering, the explicit no-access panel) the
//! browser needs a real session cookie, and producing one normally requires a
//! full OIDC login. This binary mints the same cookie a real callback would,
//! signed with a signing key you supply, so the browser can be pointed at a
//! locally running backend with the cookie injected.
//!
//! Usage:
//!
//! ```text
//! cargo run --example mint-cookie -- <signing-key> <subject> <groups>
//! ```
//!
//! The arguments:
//!
//! - `<signing-key>`: the `sessionSigningKey` the target backend is configured
//!   with. The cookie only validates against the backend whose signing key
//!   matches, so this must be the key of the instance under test.
//! - `<subject>`: the ID-token `sub` recorded in the cookie.
//! - `<groups>`: a comma-separated groups claim, for example
//!   `yafm-devs,yafm-tv`. The access evaluation filters on exactly these names.
//!
//! The output is the cookie in its header form, `yafm_session=<value>`. Inject
//! it into the browser as the `yafm_session` cookie value alone (without the
//! name prefix) for the `127.0.0.1` domain.
//!
//! The minting path is the test-only helper `mint_session_cookie_for_tests`;
//! the display name and email are fixed placeholders because only the access
//! evaluation and the identity surface read them, and the verification checks
//! behavior, not the exact claims.
//!
//! This is a development and verification tool: it is doc-hidden, linked into
//! every build but never called by the server, and has no place in the runtime
//! image.
use backend::auth::{mint_session_cookie_for_tests, SessionClaims};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let signing_key = args.get(1).expect("usage: mint-cookie <signing-key> <subject> <groups>");
    let subject = args.get(2).cloned().unwrap_or_else(|| "test-subject".to_string());
    let groups = args.get(3).cloned().unwrap_or_default();
    let claims = SessionClaims {
        subject: subject.clone(),
        display_name: "Test User".to_string(),
        email: "test-user@example.com".to_string(),
        groups: groups.split(',').map(|g| g.trim().to_string()).collect(),
    };
    let cookie = mint_session_cookie_for_tests(Some(signing_key), false, &claims);
    println!("{cookie}");
}
