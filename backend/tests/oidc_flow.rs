//! The mock-IdP end-to-end OIDC flow test: the full redirect → authorize →
//! callback → session loop against an in-test mock identity provider. The
//! gate-enumeration test (auth_gate.rs) covers rejection; this binary covers
//! the happy path the plan's acceptance calls for. The login endpoint redirects
//! to a real provider, the mock issues a code and an RS256 ID token, the
//! callback exchanges the code and validates the token, and the resulting
//! session grants access to every endpoint.
//!
//! The mock signs the ID token with openidconnect's own serialization
//! (`CoreIdToken::new`, no hand-rolled JWT) and validates the PKCE verifier at
//! the token endpoint like a real IdP does. CI never touches a live authentik:
//! the mock serves plain HTTP on an OS-assigned loopback port, which the issuer
//! validation accepts for loopback hosts (the exception this flow test exists
//! for).
//!
//! One test initializes the process-global config and auth state once (the
//! fixture-lock pattern): the OnceCell cannot be reset, so a second test would
//! race. The whole journey runs in one `#[tokio::test]` instead, and the mock
//! server is spawned on that test's own runtime.

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Form, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use openidconnect::core::{
    CoreIdToken, CoreIdTokenClaims, CoreIdTokenFields, CoreJsonWebKeySet, CoreJwsSigningAlgorithm,
    CoreRsaPrivateSigningKey, CoreTokenResponse, CoreTokenType,
};
use openidconnect::reqwest;
use openidconnect::{
    AccessToken, Audience, EmptyAdditionalClaims, EmptyExtraTokenFields, IssuerUrl, JsonWebKeyId,
    Nonce, PkceCodeChallenge, PkceCodeVerifier, PrivateSigningKey, StandardClaims,
    SubjectIdentifier,
};
use serde::Deserialize;
use serde_json::Value;
use tempfile::TempDir;
use tower::util::ServiceExt;

/// The client id the test config and the mock agree on: the ID token's
/// audience claim must match the backend's client id exactly, or the callback
/// rejects the token.
const TEST_CLIENT_ID: &str = "yafm-spa";

/// The client secret of the test config, agreed with the mock for the token
/// exchange. The session-cookie jar is no longer derived from it: the config
/// carries a dedicated `sessionSigningKey`, so the client secret alone cannot
/// mint valid sessions.
const TEST_CLIENT_SECRET: &str = "mock-idp-client-secret";

/// The dedicated cookie-signing key the test config carries. The global jar
/// is derived from it (the flow runs through the global jar), so the
/// callback's minted session stays valid for the rest of the flow.
const TEST_SIGNING_KEY: &str = "auth-test-signing-key-0123456789abcdef";

/// The `kid` the mock's JWK carries. The mock's JWKS holds one eligible key, so
/// the backend's verifier picks it without ambiguity.
const TEST_KEY_ID: &str = "test-key";

/// The `kid` of the rotated key the mock switches to when the flow test
/// exercises a signing-key rotation. The `kid` must change too: openidconnect's
/// verifier looks up a key by the `kid` in the JWT header, so an ID token
/// signed with a `kid` absent from the backend's cached JWKS is what triggers
/// the `NoMatchingKey` refresh path Finding 1 adds.
const TEST_KEY_ID_B: &str = "test-key-2";

/// A pre-generated RSA 2048 test key (`openssl genrsa -traditional 2048`),
/// thrown away: the private half signs the mock's ID tokens and the public half
/// is served through the JWKS endpoint. RS256 is the recommended authentik
/// production mode, so the mock signs through the exact path production uses.
const TEST_KEY_PEM: &str = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEpQIBAAKCAQEAx3ze3eFZX4WrvbvILwYsGvWr1VJ2vdqr7/Lo2FKyGfBcoLjX
/z5fXkW+tC4f1jBZ5Oh2gtDfHXyeaUSxKrORCaGY4/Pa6XP9KV6SfmuOxwRf/KG9
u3Z+ezKe5XTvp5chNsQUPDUgEmL2VO5kyRIXZGRUeKkYjE8zg4s6BYkAo3tU3cjl
IMVwZIzHdWAuX2XO/R8qH7xfjkvxYQrcIhzxntql85O30uIWMIiQKsE4oBtWWayh
CcO6MjeSoiXKUXD+h2i4O1t4CFTIn0YRgoOYFOReSVHcakdDJA6EOStJr1oErgjf
fp+lMYQPmiU6FpQYytcwqqZdhPmKFf4eBThFlwIDAQABAoIBAFoQEj5yQvtRShw6
70Hrs3XofE+vD1TfqMiIDn+7thTn46ncSgg+jKfvLQ4D1PPKmIs0OG0PB+w0GwDD
tojk0RJcFr6zlZ3Yc+99dv4EaU2IuB1CmHpOIQRV8k794ET1glVLaSdVhMlITJZD
mtT8ifsVIN3o2eBe0Y8OCH//Pn6Pn5u0m2LEs2P0J9ZPTW/M3fWAdNONjpTGS3IP
oSSiOuH7GzLmZUYegdjQFI/0QH1KM2BJprDSBYtO2Bl14CBdO0+81BvQA6mMs76m
T+Lrj7iE5HoQ6837Me6Ncm39t/OCsQmOS9xmJRF+fAIJ9aWDTEYOsbqpzpBboivK
xCBRzUECgYEA7EOrqRC6V+R8vYe9si+jFEqRF/MPNr3p9LNb1+CmvhCp+BfAGeSZ
5Dn64tlxJH3mLjb3YNGryMHBXpIeTmSq4PJLf4B5Lq+1hIAHI1pT3y7jaDdKkdJ6
tY/WFoCqxAFfxINh/YDDPsigPbiNx1mo/C4MFd8JKutkINqfcyszmDkCgYEA2CbD
BI5Le9JDFpu5OaRWUeHPOAlJKBoFt/FRIVv6M+8uUMdb8FRYQP03WekZZKeQ5gL2
iBRww1RAmYhPzWipOFx1Z9yFv2H2m2keEJT3UmvvD1f+uhOsBY1N18QyzxvEkWQS
x7woCrMy0X75f0VWYg9tEytzCKKziIXonjo2rE8CgYEAwFnnq+E+lMgk9nlI64T1
FFQRBJqSTFMZ4msT3xG7LwqKFr3fXDVNRQ4fQAkfoEIP4JhHlr+dR/jW4ZO8sL4s
kK8y4D9MacIL2jARn6qulgmqgvJg94+Q77iG6BMg9CraOTdt0+G9E6RrMVTLuP06
IvWqSTQoVpUGE+lp323Qt6ECgYEAiFey7d2/+WPA07L4nEZv+IhiSGt7DOOVNdjv
HwbAhR/a7DNEaA0b+ip/TqR9UwNrn9rAnUefdWZgtTfJdr0M+LNBj3kHmJf3kUI2
J6l/dCsHCXus/rzH5lyifHaSwhc236rrObgS3eT5KjJYuJIJEiO+3reqgQj4DCbD
e4Mm13sCgYEAtGhvFXuEW10JXVZuOfwSJrGF0fkl3KPsVvTeLWffrIygMpBQAL9u
Oz5+XN8USnK4lYeQtST/IyiqyttBpLS276XqFnbPlwofr0Lt8mPC4QrAZXSqiPm+
sO+uxmCswOSi5urTJqLWnjeuZ4RsTa7LdmH1hJInrNUQEr/7vbPP/ps=
-----END RSA PRIVATE KEY-----
"#;

/// A second pre-generated RSA 2048 test key (`openssl genrsa -traditional 2048`)
/// the mock switches to for the signing-key-rotation step. It carries a
/// distinct `kid` (`TEST_KEY_ID_B`), so an ID token signed with it fails the
/// backend's cached-JWKS signature check with `NoMatchingKey` — exactly what
/// triggers the one-time refresh.
const TEST_KEY_PEM_B: &str = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEAuqTm+aotonFjRufTD+vzRyxP0mwRWYeljb+TJ5QqxKw9si3M
zxSmaYrEmdTfS2i17BTbc9AkEEtxfspgGINkDEZy8O5HXcuOFiMnQmwW2Zz6gWfT
V9SIuvMpsiIhipXC3GnYWtkccBeS2eQOb19N/fOnigH7ySXyiJS8InPedRhuDKrQ
W9jYwwztSmGvGtHaqPHkXWpKcE7DgigoDIkpmfZTWMUhUbOvtVqi0+AIcg/g/Ynt
0P0xUCQFmZbkCFXmxxv/9tismwWTLUH6OpGqzXOZ85Jeif31e9teBzXnEMGKUadQ
1sm8n3zxz4P0LO1jIkL2zNHS46pv/7nVHLzxwwIDAQABAoIBABwSyKEd7T5FQtL7
9J2s9ksqyZjTY2qtggPHoHkwCpzJcYA27lrpdrxiQH8I60s65T4sxvNtB7ehuWEC
TKDzRl2oTQqbNIXRo74FrJaLjoZN28oSFVJdJ/HCuG9QPe5L52Li0sWbaXEcwpxe
dqNe2OrNNtKFyNrxB8Fuabve5MOE1pxwwP/ukGiQYWpDoFG6aYq0KnKFeYma+nY9
0/Wz9bxrwdVZh4poz18Gpx+EjQiIBRn7o1NNr+zRUd8HjqeYKqzzjtw8BaFlNO9t
J4vjVnEt/rli8tAGQVUccw3AM5tDgIW5cdGjbb8ufTdbkRS4f7hNGPKjzy+L2auc
SOiZZtkCgYEA8Lo5y+rfM8Hq/nfaX3pbyayF7D+DyWB2fymsLmUwvh4GKNJs56SD
jQpxVpqsNayptnziUsZUfZr8j9U+4TjJ54iA63dDspPr9PfiP6AZWLgU6y+GvhcV
UbDGYOXAe9liJbduz21EJX/HQ35OoHHWXW2ri1zWlqCrFvgzq8kbP4sCgYEAxnxI
Y4NO8ojlhuzjGaA1Xi3WYK4xMUrwdNN4lLUL89soWIPXkojiwBOVRIgo8McCAk0B
yqwaBDb44Lc0bVysQgKzIhyw4pDgIzyasdZffBOeVnvOH9vWzVX9FD0mLMJZQbFZ
EiQpjiyfqTSBCAvrcjAIMDGK61sOy1nFv6hP3akCgYEA1Pc7iI7KViS5e9SWiZ9b
MskBVec+9Nn1GzzHyefVvmwbcOPwWuItS4qwiDigH4AYSIylQSuateB2jdzPGzs9
TCt0OlwxtPuuZPMj4rwFkHqSbxqFrwgG4VVtu22m4yqG7O0iCDoXbsFjjO9iKglr
5w3OFKXWZj3P/qsoM1LgW08CgYB1uMTecMTkSJmJ2voe+sxsXVdm5Cm9CKtxPvOn
j3HVYkidpyS2foWuUm8XxIIzvHTOlInZgRW1Jj2aWk64Bl0MkblZJBctaavmek1t
6K2dU613sdphPuw5wSRnWpVHusVhlyQzBEu5TXIs0z0sXpV4llBk9R1l1g4CQe5t
bBBicQKBgB+o6MZtMHAkXvC+HCquzHi0f5uBe2AWG4htdnNRbrBwQZIuiyk79gJS
NsHhThT5FKsKqrpHnfWw6Eu9DOjn0bEtxK9vTzPf4NwOEV9dNoVWcis1omcZBVdY
oTIhBRLSIHb0lfsHrZXT+EAq8OMmwfCu2N1HLfr3tvq+G4CmQ7kn
-----END RSA PRIVATE KEY-----
"#;

/// The mock IdP, serving plain HTTP on an OS-assigned loopback port.
struct MockIdp {
    /// The per-provider issuer the mock publishes in its discovery document
    /// (the authentik default issuer shape, with the trailing slash). It must
    /// match the configured issuer exactly: openidconnect compares them.
    issuer: String,
    /// The mock's bound port, for the configured redirect URI.
    port: u16,
    /// Shared state with the served router, kept so the test can rotate the
    /// signing key between logins (a signing-key rotation is driven by the
    /// provider, so the test needs a handle into it).
    state: Arc<MockIdpState>,
}

impl MockIdp {
    /// Rotates the mock's signing key to `TEST_KEY_PEM_B` (a new key with a
    /// distinct `kid`). The mock is the only writer and the lock is held only
    /// for the synchronous swap, so the in-flight token/JWKS handlers are not
    /// blocked for long. Between the first and second login of the flow test
    /// this reproduces an authentik signing-key rotation: the rotated ID token
    /// carries a `kid` the backend's cached JWKS does not know.
    fn rotate_signing_key(&self) {
        let mut signing_key = self
            .state
            .signing_key
            .lock()
            .expect("lock the mock signing key");
        *signing_key = CoreRsaPrivateSigningKey::from_pem(
            TEST_KEY_PEM_B,
            Some(JsonWebKeyId::new(TEST_KEY_ID_B.to_string())),
        )
        .expect("a valid rotated RSA private key");
    }
}

/// The mock IdP's state: the RS256 signing key built once from the
/// pre-generated PEM, and the authorization codes issued by the authorize stub
/// with the challenge and nonce they were issued against.
struct MockIdpState {
    /// The RS256 signing key, behind a lock so the flow test can rotate it
    /// between logins (an authentik signing-key rotation). The lock is held
    /// only for the synchronous sign/JWKS build, never across an `.await`.
    signing_key: Mutex<CoreRsaPrivateSigningKey>,
    issuer: String,
    codes: Mutex<MockIdpCodes>,
}

/// The authorization codes handed out by the authorize stub, keyed by the code
/// string. Codes are single-use at the token endpoint, like a real IdP.
#[derive(Default)]
struct MockIdpCodes {
    next_code: u64,
    issued: HashMap<String, IssuedCode>,
}

/// What an issued code carries across the authorize → token round trip: the
/// PKCE challenge and the nonce from the authorization request, which the
/// token endpoint validates before issuing the ID token.
struct IssuedCode {
    code_challenge: String,
    nonce: String,
}

/// Starts the mock IdP: a real HTTP server on 127.0.0.1:0, served from a
/// spawned task on the test's runtime. The listener is bound synchronously so
/// the port is known before the config is written; connections arriving before
/// the serve loop starts queue in the OS backlog, so there is no race.
fn start_mock_idp() -> MockIdp {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the mock IdP port");
    let port = listener.local_addr().expect("the bound address").port();
    listener
        .set_nonblocking(true)
        .expect("set the mock IdP listener non-blocking");
    let issuer = format!("http://127.0.0.1:{port}/application/o/yafm/");
    let state = Arc::new(MockIdpState {
        signing_key: Mutex::new(
            CoreRsaPrivateSigningKey::from_pem(
                TEST_KEY_PEM,
                Some(JsonWebKeyId::new(TEST_KEY_ID.to_string())),
            )
            .expect("a valid RSA private key"),
        ),
        issuer: issuer.clone(),
        codes: Mutex::new(MockIdpCodes::default()),
    });
    let state_for_router = state.clone();
    tokio::spawn(async move {
        let listener =
            tokio::net::TcpListener::from_std(listener).expect("convert the mock IdP listener");
        axum::serve(listener, mock_idp_router(state_for_router))
            .await
            .expect("serve the mock IdP");
    });
    MockIdp {
        issuer,
        port,
        state,
    }
}

/// The mock IdP's routes, at the authentik per-provider paths. The discovery
/// URL the backend fetches is `{issuer}.well-known/openid-configuration`, so
/// the discovery route carries the provider's slug path exactly.
fn mock_idp_router(state: Arc<MockIdpState>) -> Router {
    Router::new()
        .route(
            "/application/o/yafm/.well-known/openid-configuration",
            get(mock_discovery),
        )
        .route("/application/o/yafm/authorize/", get(mock_authorize))
        .route("/application/o/yafm/token/", post(mock_token))
        .route("/application/o/yafm/jwks/", get(mock_jwks))
        .route("/application/o/yafm/end-session/", get(mock_end_session))
        .with_state(state)
}

/// The discovery document. The issuer must match the configured value exactly
/// (openidconnect compares them); the jwks_uri and token endpoint point at the
/// mock's own routes; RS256 is declared because that is what the mock signs
/// with, and the backend's verifier only allows the declared algorithms.
async fn mock_discovery(State(state): State<Arc<MockIdpState>>) -> Response {
    let issuer = state.issuer.as_str();
    axum::Json(serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}authorize/"),
        "token_endpoint": format!("{issuer}token/"),
        "jwks_uri": format!("{issuer}jwks/"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "end_session_endpoint": format!("{issuer}end-session/"),
    }))
    .into_response()
}

/// The authorization request's query parameters, as the wire format spells
/// them (snake_case, per the OAuth2 spec).
#[derive(Debug, Deserialize)]
struct MockAuthorizeRequest {
    response_type: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    redirect_uri: Option<String>,
    nonce: Option<String>,
}

/// The authorize stub: a real IdP rejects a malformed authorization request, so
/// the mock validates the parameters the backend's built URL must carry. It
/// then issues a code, remembers the challenge and nonce it was issued against,
/// and redirects back to the configured redirect URI with code + state.
async fn mock_authorize(
    State(state): State<Arc<MockIdpState>>,
    Query(request): Query<MockAuthorizeRequest>,
) -> Response {
    let response_type = request.response_type.as_deref();
    let state_param = request.state.as_deref().unwrap_or_default();
    let nonce = request.nonce.as_deref().unwrap_or_default();
    let challenge = request.code_challenge.as_deref().unwrap_or_default();
    let redirect_uri = request.redirect_uri.as_deref().unwrap_or_default();
    if response_type != Some("code")
        || state_param.is_empty()
        || nonce.is_empty()
        || challenge.is_empty()
        || request.code_challenge_method.as_deref() != Some("S256")
        || redirect_uri.is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            "the mock IdP rejects a malformed authorization request",
        )
            .into_response();
    }
    let code = {
        let mut codes = state.codes.lock().expect("lock the mock IdP codes");
        let code = format!("mock-code-{}", codes.next_code);
        codes.next_code += 1;
        codes.issued.insert(
            code.clone(),
            IssuedCode {
                code_challenge: challenge.to_string(),
                nonce: nonce.to_string(),
            },
        );
        code
    };
    let location = format!("{redirect_uri}?code={code}&state={state_param}");
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

/// The token request's form body, as the wire format carries it (RFC 7636's
/// `code_verifier`, plus the grant type and the code).
#[derive(Debug, Deserialize)]
struct MockTokenRequest {
    grant_type: String,
    code: String,
    code_verifier: String,
}

/// The token stub: validates the grant type, consumes the code (single-use),
/// checks the PKCE verifier against the challenge from the authorization
/// request, and issues an RS256 ID token signed with the pre-generated key.
/// The ID token is minted with openidconnect's own serialization, so the
/// backend validates the exact token shape production issues.
async fn mock_token(
    State(state): State<Arc<MockIdpState>>,
    Form(body): Form<MockTokenRequest>,
) -> Response {
    if body.grant_type != "authorization_code" {
        return (
            StatusCode::BAD_REQUEST,
            "the mock IdP only issues authorization codes",
        )
            .into_response();
    }
    let issued = state
        .codes
        .lock()
        .expect("lock the mock IdP codes")
        .issued
        .remove(&body.code);
    let Some(issued) = issued else {
        return (
            StatusCode::BAD_REQUEST,
            "the mock IdP does not recognize the authorization code",
        )
            .into_response();
    };
    // PKCE: the S256 challenge of the received verifier must match the
    // challenge the code was issued against (RFC 7636's token endpoint check).
    let challenge =
        PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(body.code_verifier));
    if challenge.as_str() != issued.code_challenge {
        return (
            StatusCode::BAD_REQUEST,
            "the PKCE verifier does not match the challenge",
        )
            .into_response();
    }
    let claims = CoreIdTokenClaims::new(
        IssuerUrl::new(state.issuer.clone()).expect("a valid issuer URL"),
        // The audience is the client id; the backend's verifier rejects the
        // token unless it matches.
        vec![Audience::new(TEST_CLIENT_ID.to_string())],
        // ID token expiration: shorter than the session, like a real provider.
        Utc::now() + Duration::seconds(300),
        Utc::now(),
        StandardClaims::new(SubjectIdentifier::new("test-user".to_string())),
        EmptyAdditionalClaims {},
    );
    // The nonce claim must match the nonce the backend stored in the login
    // cookie (the authorize request carried it). email_verified is
    // deliberately NOT set: the backend does not validate it (authentik
    // defaults it to false since 2025.10).
    let claims = claims.set_nonce(Some(Nonce::new(issued.nonce)));
    let id_token = {
        let signing_key = state.signing_key.lock().expect("lock the mock signing key");
        CoreIdToken::new(
            claims,
            &*signing_key,
            CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
            None,
            None,
        )
        .expect("the mock IdP signs the ID token")
    };
    axum::Json(CoreTokenResponse::new(
        AccessToken::new("mock-access-token".to_string()),
        CoreTokenType::Bearer,
        CoreIdTokenFields::new(Some(id_token), EmptyExtraTokenFields {}),
    ))
    .into_response()
}

/// The mock's JWKS document: the public half of the pre-generated key, built
/// through openidconnect's own serialization.
async fn mock_jwks(State(state): State<Arc<MockIdpState>>) -> Response {
    let signing_key = state.signing_key.lock().expect("lock the mock signing key");
    let jwks = CoreJsonWebKeySet::new(vec![signing_key.as_verification_key()]);
    axum::Json(jwks).into_response()
}

/// The mock's end-session page. The logout test asserts the redirect target,
/// not this page; a real browser would land here after logout.
async fn mock_end_session() -> impl IntoResponse {
    "the mock IdP end-session page"
}

async fn get_backend(app: &Router, path: &str, cookie_header: Option<&str>) -> Response {
    let mut builder = axum::http::Request::builder().method("GET").uri(path);
    if let Some(value) = cookie_header {
        builder = builder.header("Cookie", value);
    }
    app.clone()
        .oneshot(
            builder
                .body(axum::body::Body::empty())
                .expect("build request"),
        )
        .await
        .expect("response")
}

/// The mock IdP is a real HTTP server, so the browser-side steps that navigate
/// to the provider run through a real HTTP client (redirects disabled, so the
/// 302 is inspectable instead of followed).
async fn mock_get(url: &str) -> reqwest::Response {
    reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("the mock HTTP client builds")
        .get(url)
        .send()
        .await
        .expect("the mock IdP responds")
}

/// The `Cookie` request-header value a browser echoes back for the named
/// cookie: the first `;`-separated segment of the matching Set-Cookie header
/// (the percent-encoded name=value a browser echoes verbatim). The backend
/// percent-decodes it on the next request.
fn cookie_header_value(response: &Response, name: &str) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| {
            value
                .split(';')
                .next()
                .and_then(|pair| pair.split_once('='))
                .is_some_and(|(cookie_name, _)| cookie_name.trim() == name)
        })
        .and_then(|value| value.split(';').next())
        .expect("the response carries the named Set-Cookie header")
        .trim()
        .to_string()
}

/// All Set-Cookie headers of a response, joined for substring assertions.
fn set_cookie_headers(response: &Response) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join("; ")
}

/// The Location header of a redirect response. Takes the header map so both
/// the backend's responses (tower oneshot) and the mock IdP's real HTTP
/// responses (the re-exported reqwest) feed the same helper.
fn location_header(headers: &header::HeaderMap) -> String {
    headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("the redirect carries a Location header")
        .to_string()
}

/// The value of a query parameter in a URL. The query string is split on '?'
/// and '&'; the values this test reads (the state, the code) are
/// alphanumeric/url-safe, so no percent-decoding is needed.
fn query_parameter(url: &str, name: &str) -> Option<String> {
    url.split('?')
        .nth(1)?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_string())
}

/// The full OIDC round trip against the mock (ISC-2, ISC-3, ISC-4): the gate
/// redirects the unauthenticated SPA shell to the login endpoint, the login
/// endpoint 302s the browser to the mock's authorization endpoint with the
/// signed login cookie, the mock issues a code and redirects back with
/// code + state, the callback exchanges the code (with the PKCE verifier from
/// the login cookie) and validates the RS256 ID token (issuer, audience,
/// expiration, nonce) before issuing the session, and the session grants
/// access to every endpoint. Logout closes the journey at the mock's
/// end-session endpoint.
#[tokio::test]
async fn the_full_oidc_flow_mints_a_session_and_closes_the_loop() {
    // One auth state per process: the config and the Oidc state are
    // initialized once here (the fixture-lock pattern).
    let mock = start_mock_idp();
    let temp = TempDir::new().expect("temp dir");
    let shared_root = temp.path().join("share");
    std::fs::create_dir_all(shared_root.join("docs")).expect("create the docs dir");
    std::fs::write(shared_root.join("docs").join("demo.txt"), "hello world")
        .expect("write the demo file");
    let config_path = temp.path().join("yafm.config.yaml");
    std::fs::write(
        &config_path,
        format!(
            "sharedRoot: {}\nshowHidden: false\noidc:\n  issuer: {}\n  clientId: {TEST_CLIENT_ID}\n  clientSecret: {TEST_CLIENT_SECRET}\n  sessionSigningKey: {TEST_SIGNING_KEY}\n  redirectUri: http://127.0.0.1:{}/api/v1/auth/callback\n  cookieSecure: false\n",
            shared_root.display(),
            mock.issuer,
            mock.port
        ),
    )
    .expect("write the test config");
    backend::initialize_app_config(&config_path).expect("initialize the app config");
    backend::auth::initialize_auth().expect("initialize the Oidc auth state");
    let app = backend::app_router();

    // 1. The gate redirects the unauthenticated SPA shell to the login
    //    endpoint (the hybrid gate: browser navigations get a 302, not a 401).
    let response = get_backend(&app, "/", None).await;
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "step 1: the gate redirects the unauthenticated SPA shell"
    );
    let login_location = location_header(response.headers());
    assert!(
        login_location.starts_with(backend::auth::LOGIN_PATH),
        "step 1: the gate targets the login endpoint, got {login_location}"
    );

    // 2. The login endpoint 302s the browser to the mock's authorization
    //    endpoint with the short-lived signed login cookie set.
    let response = get_backend(&app, &login_location, None).await;
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "step 2: the login endpoint redirects to the provider"
    );
    let authorize_location = location_header(response.headers());
    assert!(
        authorize_location.starts_with(&format!("{}authorize/", mock.issuer)),
        "step 2: the login redirect targets the mock's authorization endpoint, got {authorize_location}"
    );
    let login_cookie = cookie_header_value(&response, backend::auth::LOGIN_COOKIE_NAME);
    assert!(
        !login_cookie.is_empty(),
        "step 2: the login endpoint sets the login cookie"
    );

    // 3. The browser authenticates at the mock: the authorization request the
    //    backend built is accepted (the mock validates response_type, state,
    //    nonce, the PKCE challenge and the redirect URI), a code is issued and
    //    the browser is sent back to the configured redirect URI.
    let mock_response = mock_get(&authorize_location).await;
    assert_eq!(
        mock_response.status(),
        StatusCode::FOUND,
        "step 3: the mock accepts the authorization request and issues a code"
    );
    let callback_location = location_header(mock_response.headers());
    let code = query_parameter(&callback_location, "code")
        .expect("step 3: the mock's redirect carries the code");
    let state = query_parameter(&authorize_location, "state")
        .expect("step 3: the authorize URL carries the state");
    assert_eq!(
        query_parameter(&callback_location, "state"),
        Some(state.clone()),
        "step 3: the mock echoes the state back"
    );

    // 4. The callback validates the login cookie and the state, exchanges the
    //    code at the mock's token endpoint (server-to-server, with the PKCE
    //    verifier from the login cookie), validates the RS256 ID token against
    //    the mock's JWKS and issues the session with a 302 to the returnTo.
    let callback_path = format!("/api/v1/auth/callback?code={code}&state={state}");
    let response = get_backend(&app, &callback_path, Some(&login_cookie)).await;
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "step 4: the callback exchanges the code and issues the session"
    );
    assert_eq!(
        location_header(response.headers()),
        "/",
        "step 4: the callback redirects to the validated returnTo"
    );
    let session_cookie = cookie_header_value(&response, backend::auth::SESSION_COOKIE_NAME);
    assert!(
        !session_cookie.is_empty(),
        "step 4: the callback issues the session cookie"
    );
    let set_cookies = set_cookie_headers(&response);
    assert!(
        set_cookies.contains("yafm_oidc_login=") && set_cookies.contains("Max-Age=0"),
        "step 4: the callback clears the login cookie in the same response: {set_cookies}"
    );

    // 5. The session grants access to every endpoint, so the loop closes.
    let response = get_backend(&app, "/api/v1/health", Some(&session_cookie)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "step 5: the session grants access to /api/v1/health"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json: Value = serde_json::from_slice(&bytes).expect("the health body is JSON");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "backend");

    let response = get_backend(&app, "/api/v1/directory?p=docs", Some(&session_cookie)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "step 5: the session grants access to /api/v1/directory"
    );

    let response = get_backend(
        &app,
        "/api/v1/download?p=docs/demo.txt",
        Some(&session_cookie),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "step 5: the session grants access to /api/v1/download"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    assert_eq!(&bytes[..], b"hello world", "the file's content is served");

    let response = get_backend(&app, "/", Some(&session_cookie)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "step 5: the session grants access to the SPA shell"
    );

    // 6. Logout redirects to the mock's end-session endpoint (the discovered
    //    value) and clears the session cookie, so the journey ends cleanly.
    let response = get_backend(&app, "/api/v1/auth/logout", Some(&session_cookie)).await;
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "step 6: logout redirects to the provider"
    );
    let end_session = location_header(response.headers());
    assert!(
        end_session.starts_with(&format!("{}end-session/", mock.issuer)),
        "step 6: logout targets the mock's end-session endpoint, got {end_session}"
    );
    let set_cookies = set_cookie_headers(&response);
    assert!(
        set_cookies.contains("yafm_session=") && set_cookies.contains("Max-Age=0"),
        "step 6: logout clears the session cookie: {set_cookies}"
    );

    // 7. Signing-key rotation (Finding 1): the mock swaps its signing key (and
    //    `kid`) so a fresh login produces an ID token signed with a key the
    //    backend's cached JWKS (fetched at the first discovery) does not know.
    //    The callback detects the stale key (`NoMatchingKey`), clears the
    //    cache, re-discovers the JWKS once and retries — proving the
    //    refresh-on-rotation path. Without it, this second login would fail
    //    with a 401.
    mock.rotate_signing_key();

    let response = get_backend(&app, backend::auth::LOGIN_PATH, None).await;
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "step 7: the login endpoint redirects to the (rotated) provider"
    );
    let authorize_location = location_header(response.headers());
    assert!(
        authorize_location.starts_with(&format!("{}authorize/", mock.issuer)),
        "step 7: the login redirect targets the mock's authorization endpoint, got {authorize_location}"
    );
    let login_cookie = cookie_header_value(&response, backend::auth::LOGIN_COOKIE_NAME);
    assert!(
        !login_cookie.is_empty(),
        "step 7: the login endpoint sets the login cookie"
    );

    let mock_response = mock_get(&authorize_location).await;
    assert_eq!(
        mock_response.status(),
        StatusCode::FOUND,
        "step 7: the mock accepts the authorization request and issues a code"
    );
    let callback_location = location_header(mock_response.headers());
    let code = query_parameter(&callback_location, "code")
        .expect("step 7: the mock's redirect carries the code");
    let state = query_parameter(&authorize_location, "state")
        .expect("step 7: the authorize URL carries the state");
    assert_eq!(
        query_parameter(&callback_location, "state"),
        Some(state.clone()),
        "step 7: the mock echoes the state back"
    );

    // The callback first fails the signature check against the cached JWKS,
    // then refreshes the cache and retries once, so it still issues a session.
    let callback_path = format!("/api/v1/auth/callback?code={code}&state={state}");
    let response = get_backend(&app, &callback_path, Some(&login_cookie)).await;
    assert_eq!(
        response.status(),
        StatusCode::FOUND,
        "step 7: the callback refreshes the JWKS on rotation and still issues a session"
    );
    assert_eq!(
        location_header(response.headers()),
        "/",
        "step 7: the refreshed callback redirects to the validated returnTo"
    );
    let session_cookie = cookie_header_value(&response, backend::auth::SESSION_COOKIE_NAME);
    assert!(
        !session_cookie.is_empty(),
        "step 7: the refreshed callback issues a session cookie"
    );
}
