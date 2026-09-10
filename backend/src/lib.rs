use std::ffi::OsStr;
use std::fs;
use std::io::{ErrorKind, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{OriginalUri, Query};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use once_cell::sync::OnceCell;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

// The filesystem layer relies on std::os::unix APIs (dev/ino identity checks and
// raw-byte path handling) and targets Linux, where the runtime image and CI run.
#[cfg(not(unix))]
compile_error!("yet-another-file-manager targets unix (Linux); non-unix builds are not supported.");
use tower_http::trace::TraceLayer;
use utoipa::{OpenApi, ToSchema};

#[derive(Embed)]
#[folder = "../frontend/dist/"]
#[allow_missing = true]
struct FrontendAssets;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicErrorResponse {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub entries: Vec<DirectoryEntry>,
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(rename = "sharedRoot")]
    shared_root: String,
    #[serde(rename = "showHidden")]
    show_hidden: Option<bool>,
    #[serde(rename = "listenAddress")]
    listen_address: Option<String>,
    #[serde(rename = "listenPort")]
    listen_port: Option<u16>,
}

#[derive(Debug, Clone)]
struct AppConfig {
    shared_root: PathBuf,
    show_hidden: bool,
    listen_address: String,
    listen_port: u16,
}

#[derive(Debug, Deserialize)]
struct PathQuery {
    #[serde(default)]
    p: String,
}

#[derive(Debug)]
enum ApiErrorKind {
    BadRequest,
    NotFound,
    Unavailable,
}

#[derive(Debug)]
struct ApiError {
    kind: ApiErrorKind,
    message: String,
}

impl ApiError {
    fn bad_request(message: &str) -> Self {
        Self {
            kind: ApiErrorKind::BadRequest,
            message: message.to_string(),
        }
    }

    fn not_found(message: &str) -> Self {
        Self {
            kind: ApiErrorKind::NotFound,
            message: message.to_string(),
        }
    }

    fn unavailable(message: &str) -> Self {
        Self {
            kind: ApiErrorKind::Unavailable,
            message: message.to_string(),
        }
    }

    fn status(&self) -> StatusCode {
        match self.kind {
            ApiErrorKind::BadRequest => StatusCode::BAD_REQUEST,
            ApiErrorKind::NotFound => StatusCode::NOT_FOUND,
            ApiErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status(),
            Json(PublicErrorResponse {
                message: self.message,
            }),
        )
            .into_response()
    }
}

static CONFIG: OnceCell<AppConfig> = OnceCell::new();

#[derive(Debug, Clone)]
pub struct ServerBinding {
    pub address: String,
    pub port: u16,
}

fn parse_config_file(raw: &str, config_path: &Path) -> Result<ConfigFile, String> {
    let extension = config_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_lowercase();

    match extension.as_str() {
        "json" => {
            serde_json::from_str(raw).map_err(|_| "Configuration file is invalid".to_string())
        }
        "yaml" | "yml" => {
            serde_yaml::from_str(raw).map_err(|_| "Configuration file is invalid".to_string())
        }
        _ => serde_yaml::from_str(raw)
            .or_else(|_| serde_json::from_str(raw))
            .map_err(|_| "Configuration file is invalid".to_string()),
    }
}

fn load_app_config(config_path: &Path) -> Result<AppConfig, String> {
    let raw = fs::read_to_string(config_path)
        .map_err(|_| "Configuration file could not be read".to_string())?;
    let parsed = parse_config_file(&raw, config_path)?;

    if parsed.shared_root.trim().is_empty() {
        return Err("Configuration sharedRoot is required".to_string());
    }

    let shared_root = PathBuf::from(parsed.shared_root.trim());
    let shared_root_display = shared_root.display().to_string();

    if !shared_root.exists() {
        return Err(format!(
            "Configuration sharedRoot \"{shared_root_display}\" does not exist; create it or fix the config file"
        ));
    }

    let canonical_root = fs::canonicalize(&shared_root)
        .map_err(|_| format!("Configuration sharedRoot \"{shared_root_display}\" is invalid"))?;
    if !canonical_root.is_dir() {
        return Err(format!(
            "Configuration sharedRoot \"{shared_root_display}\" must be a directory"
        ));
    }

    let listen_address = parsed
        .listen_address
        .unwrap_or_else(|| "0.0.0.0".to_string())
        .trim()
        .to_string();
    if listen_address.is_empty() {
        return Err("Configuration listenAddress must be a non-empty string".to_string());
    }

    Ok(AppConfig {
        shared_root: canonical_root,
        show_hidden: parsed.show_hidden.unwrap_or(false),
        listen_address,
        listen_port: parsed.listen_port.unwrap_or(8080),
    })
}

pub fn initialize_app_config(config_path: &Path) -> Result<(), String> {
    CONFIG
        .get_or_try_init(|| load_app_config(config_path))
        .map(|_| ())
}

pub fn get_server_binding() -> Result<ServerBinding, String> {
    let config = CONFIG
        .get()
        .ok_or_else(|| "Application configuration was not initialized".to_string())?;

    Ok(ServerBinding {
        address: config.listen_address.clone(),
        port: config.listen_port,
    })
}

fn get_app_config() -> Result<&'static AppConfig, ApiError> {
    CONFIG
        .get()
        .ok_or_else(|| ApiError::unavailable("The shared directory is unavailable."))
}

fn validate_relative_path(input: &str) -> Result<String, ApiError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.contains('\0') || trimmed.contains('\\') || trimmed.starts_with('/') {
        return Err(ApiError::bad_request("The requested path is invalid."));
    }

    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_string_lossy().to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(ApiError::not_found(
                    "The requested directory could not be found.",
                ));
            }
            _ => return Err(ApiError::bad_request("The requested path is invalid.")),
        }
    }

    Ok(parts.join("/"))
}

fn ensure_within_root(root: &Path, candidate: &Path) -> Result<(), ApiError> {
    if candidate == root || candidate.starts_with(root) {
        Ok(())
    } else {
        Err(ApiError::not_found(
            "The requested directory could not be found.",
        ))
    }
}

fn parent_path(current: &str) -> Option<String> {
    if current.is_empty() {
        return None;
    }
    if let Some((parent, _)) = current.rsplit_once('/') {
        return Some(parent.to_string());
    }
    Some(String::new())
}

fn classify_entry(meta: &fs::Metadata) -> EntryKind {
    if meta.is_dir() {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}

fn modified_to_iso(meta: &fs::Metadata) -> Option<String> {
    let modified = meta.modified().ok()?;
    let dt: DateTime<Utc> = modified.into();
    Some(dt.to_rfc3339())
}

/// Renders an entry name for the listing API. Valid UTF-8 names are passed through
/// unchanged; names that are not valid UTF-8 cannot be represented in JSON, so their
/// raw bytes are percent-encoded instead. The encoded form is decodable by
/// `percent_decode_relative_path`, which lets such entries be re-opened through the API.
fn percent_encode_entry_name(raw: &OsStr) -> String {
    let bytes = raw.as_bytes();
    if let Ok(name) = std::str::from_utf8(bytes) {
        return name.to_string();
    }

    let mut encoded = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte.is_ascii_graphic() && byte != b'%' && byte != b'\\' && byte != b'/' {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Percent-decodes one string into raw bytes. Malformed escapes (`%` not followed by
/// two hex digits) are passed through literally, matching the WHATWG decode behavior.
fn percent_decode_bytes(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%'
            && let (Some(high), Some(low)) = (
                bytes
                    .get(index + 1)
                    .and_then(|byte| char::from(*byte).to_digit(16)),
                bytes
                    .get(index + 2)
                    .and_then(|byte| char::from(*byte).to_digit(16)),
            )
        {
            decoded.push((high * 0x10 + low) as u8);
            index += 3;
            continue;
        }
        decoded.push(byte);
        index += 1;
    }
    decoded
}

/// Builds the filesystem path for a validated relative path. Path components listed
/// with a percent-encoded name (entries whose names are not valid UTF-8) are decoded
/// back to their raw bytes so they resolve to the original file. The decoded bytes are
/// re-validated because an escape could otherwise hide a `/`, `\\`, NUL or `..` that
/// `validate_relative_path` never saw.
fn percent_decode_relative_path(relative: &str) -> Result<PathBuf, ApiError> {
    let mut decoded = PathBuf::new();
    for component in relative.split('/') {
        let bytes = percent_decode_bytes(component);
        if bytes.contains(&b'/') || bytes.contains(&b'\\') || bytes.contains(&0) {
            return Err(ApiError::bad_request("The requested path is invalid."));
        }
        if bytes == b".." {
            return Err(ApiError::not_found(
                "The requested directory could not be found.",
            ));
        }
        if !bytes.is_empty() {
            decoded.push(OsStr::from_bytes(&bytes));
        }
    }
    Ok(decoded)
}

/// Canonicalizes a validated relative path under the shared root, mapping filesystem
/// errors to API errors without leaking host paths. When the literal form does not
/// exist, the percent-decoded form is tried as well so entries listed with an encoded
/// name resolve back to their original file.
async fn canonical_entry_target(
    root: &Path,
    relative: &str,
    not_found_message: &str,
) -> Result<PathBuf, ApiError> {
    let candidate = root.join(relative);
    match tokio::fs::canonicalize(&candidate).await {
        Ok(canonical) => Ok(canonical),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let decoded = percent_decode_relative_path(relative)?;
            tokio::fs::canonicalize(root.join(decoded))
                .await
                .map_err(|error| {
                    if error.kind() == ErrorKind::NotFound {
                        ApiError::not_found(not_found_message)
                    } else {
                        ApiError::unavailable("The shared directory is unavailable.")
                    }
                })
        }
        Err(_) => Err(ApiError::unavailable(
            "The shared directory is unavailable.",
        )),
    }
}

/// Maximum chunk size for streaming download bodies; bounds memory regardless of
/// file size.
const DOWNLOAD_STREAM_CAPACITY: usize = 64 * 1024;

/// A parsed single byte range with inclusive bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ByteRange {
    start: u64,
    end: u64,
}

impl ByteRange {
    fn length(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// What the `Range` header asks the server to do for a representation of `total` bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeDecision {
    /// No usable `Range` header (absent, unsupported unit, malformed or multi-range):
    /// serve the full representation.
    Full,
    /// A satisfiable single range: serve 206 with this range.
    Partial(ByteRange),
    /// No byte of the representation can be served: respond 416.
    NotSatisfiable,
}

/// Parses a single `bytes` range spec (`N-`, `N-M`, `-N`) against a representation of
/// `total` bytes. An unsupported unit, a malformed spec or a multi-range spec
/// (comma-separated) is ignored, per HTTP leniency; the caller then serves everything.
fn parse_single_byte_range(value: &str, total: u64) -> RangeDecision {
    let Some((unit, spec)) = value.split_once('=') else {
        return RangeDecision::Full;
    };
    if !unit.eq_ignore_ascii_case("bytes") {
        return RangeDecision::Full;
    }
    let spec = spec.trim();

    if total == 0 {
        // An empty representation has no satisfiable range.
        return RangeDecision::NotSatisfiable;
    }
    if spec.contains(',') {
        return RangeDecision::Full;
    }

    if let Some(suffix) = spec.strip_prefix('-') {
        let Ok(length) = suffix.trim().parse::<u64>() else {
            return RangeDecision::Full;
        };
        if length == 0 {
            return RangeDecision::NotSatisfiable;
        }
        return RangeDecision::Partial(ByteRange {
            start: total.saturating_sub(length),
            end: total - 1,
        });
    }

    let Some((first, last)) = spec.split_once('-') else {
        return RangeDecision::Full;
    };
    let Ok(start) = first.trim().parse::<u64>() else {
        return RangeDecision::Full;
    };
    if start >= total {
        return RangeDecision::NotSatisfiable;
    }
    match last.trim().parse::<u64>() {
        // Open-ended "N-" reaches the final byte.
        Err(_) if last.trim().is_empty() => RangeDecision::Partial(ByteRange {
            start,
            end: total - 1,
        }),
        // "N-M" is clamped to the final byte of the representation.
        Ok(end) if end >= start => RangeDecision::Partial(ByteRange {
            start,
            end: end.min(total - 1),
        }),
        // "N-M" with first > last is an invalid spec; ignore the header.
        _ => RangeDecision::Full,
    }
}

/// Computes a weak validator for the file from its modification time and length,
/// formatted as `W/"<mtime>-<length>"` in hex. Returns None when the mtime cannot be
/// read, in which case conditional request headers are not honored.
fn compute_weak_etag(meta: &fs::Metadata) -> Option<HeaderValue> {
    let modified = meta.modified().ok()?;
    let nanos = modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let len = meta.len();
    header::HeaderValue::from_str(&format!("W/\"{nanos:x}-{len:x}\"")).ok()
}

/// Extracts the opaque value of an entity tag, ignoring its weak marker and quotes:
/// `W/"1a-5"` and `"1a-5"` share the opaque value `1a-5`.
fn etag_opaque_value(tag: &str) -> &str {
    let trimmed = tag.trim();
    let without_weakness = trimmed.strip_prefix("W/").unwrap_or(trimmed);
    without_weakness.trim_matches('"')
}

/// Compares two entity tags weakly: the weak marker does not affect the match.
fn etag_matches_weakly(candidate: &str, current: &str) -> bool {
    etag_opaque_value(candidate) == etag_opaque_value(current)
}

/// True when an `If-None-Match` header matches the current validator: `*` matches any
/// existing representation, and a comma-separated list matches weakly per entry.
fn if_none_match_matches(header_value: &str, current_etag: &str) -> bool {
    let value = header_value.trim();
    if value == "*" {
        return true;
    }
    value
        .split(',')
        .any(|tag| etag_matches_weakly(tag, current_etag))
}

/// True when an `If-Range` header matches the current validator. The header carries
/// either an entity tag or an HTTP-date (compared with the file's mtime at second
/// resolution); a non-matching or unrecognized value means the range is not served.
fn if_range_matches(
    header_value: &str,
    current_etag: Option<&str>,
    modified: Option<SystemTime>,
) -> bool {
    let value = header_value.trim();
    if value.starts_with("W/") || value.starts_with('"') {
        return current_etag.is_some_and(|etag| etag_matches_weakly(value, etag));
    }

    let Some(modified) = modified else {
        return false;
    };
    let mtime: DateTime<Utc> = modified.into();
    DateTime::parse_from_str(value, "%a, %d %b %Y %H:%M:%S GMT")
        .map(|parsed| parsed.timestamp() == mtime.timestamp())
        .unwrap_or(false)
}

/// Opens the download target and returns the file handle together with the handle's
/// own metadata, the fstat of what will actually be served. The canonical target's
/// identity is captured before the open, and the opened handle must be a regular file
/// whose (dev, ino) matches that identity: a path swapped between the stat and the
/// open is rejected with 404 instead of served. The narrower canonicalize-to-stat
/// window (a full fix would need openat2 with RESOLVE_IN_ROOT) is a known future
/// refinement.
async fn open_download_target(
    canonical_target: &Path,
) -> Result<(tokio::fs::File, fs::Metadata), ApiError> {
    let expected = tokio::fs::metadata(canonical_target)
        .await
        .map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                ApiError::not_found("The requested file could not be found.")
            } else {
                ApiError::unavailable("The shared directory is unavailable.")
            }
        })?;
    let file = tokio::fs::File::open(canonical_target)
        .await
        .map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                ApiError::not_found("The requested file could not be found.")
            } else {
                ApiError::unavailable("The shared directory is unavailable.")
            }
        })?;
    let meta = file
        .metadata()
        .await
        .map_err(|_| ApiError::unavailable("The shared directory is unavailable."))?;

    // Verify-after-open: the handle being served must be a regular file matching the
    // identity that validation approved. A mismatch means the path changed between
    // the metadata read and the open, so the opened file is not served.
    if !meta.is_file() || meta.dev() != expected.dev() || meta.ino() != expected.ino() {
        return Err(ApiError::not_found(
            "The requested file could not be found.",
        ));
    }

    Ok((file, meta))
}

/// Builds the common content headers for a 200 or 206 download response: media type,
/// attachment disposition, nosniff, range support advertisement and the weak ETag.
fn content_response_headers(file_name: &str, etag: Option<&HeaderValue>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        content_disposition_value(file_name),
    );
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        header::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::ACCEPT_RANGES,
        header::HeaderValue::from_static("bytes"),
    );
    if let Some(etag) = etag {
        headers.insert(header::ETAG, etag.clone());
    }
    headers
}

/// Builds the `Content-Disposition` value for a download. Names that fit a header
/// value as-is keep the plain `attachment; filename="..."` form; non-ASCII names
/// cannot be represented interoperably in a header value, so they get the RFC 5987
/// companion instead of the bare `attachment` fallback: an ASCII-safe percent-encoded
/// `filename` plus `filename*=UTF-8''...`. The caller sanitizes CR/LF/quote/backslash
/// before this builder runs.
fn content_disposition_value(file_name: &str) -> HeaderValue {
    if file_name.is_ascii() {
        return header::HeaderValue::from_str(&format!("attachment; filename=\"{file_name}\""))
            .unwrap_or_else(|_| header::HeaderValue::from_static("attachment"));
    }

    let encoded = percent_encode_ext_value(file_name);
    header::HeaderValue::from_str(&format!(
        "attachment; filename=\"{encoded}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| header::HeaderValue::from_static("attachment"))
}

/// Percent-encodes a name into the ASCII-safe form carried by the RFC 5987
/// `filename*` ext-value and the plain `filename` fallback. Only the characters RFC
/// 5987 allows unescaped (its attr-char set) pass through; every other byte,
/// including the UTF-8 bytes of non-ASCII names, is emitted as `%XX`.
fn percent_encode_ext_value(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len());
    for &byte in name.as_bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[utoipa::path(
    get,
    path = "/api/v1/health",
    operation_id = "getApiV1Health",
    responses((status = 200, description = "API health status", body = HealthResponse))
)]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        service: "backend".to_string(),
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/directory",
    operation_id = "getApiV1Directory",
    params(("p" = Option<String>, Query, description = "Relative path under shared root")),
    responses(
        (status = 200, description = "Directory listing", body = DirectoryListing),
        (status = 400, description = "Invalid request", body = PublicErrorResponse),
        (status = 404, description = "Directory not found", body = PublicErrorResponse),
        (status = 503, description = "Storage unavailable", body = PublicErrorResponse)
    )
)]
async fn directory(Query(query): Query<PathQuery>) -> Result<Json<DirectoryListing>, ApiError> {
    let config = get_app_config()?;
    let relative = validate_relative_path(&query.p)?;

    let canonical = canonical_entry_target(
        &config.shared_root,
        &relative,
        "The requested directory could not be found.",
    )
    .await?;

    ensure_within_root(&config.shared_root, &canonical)?;

    if !canonical.is_dir() {
        return Err(ApiError::not_found(
            "The requested directory could not be found.",
        ));
    }

    let mut entries: Vec<DirectoryEntry> = Vec::new();
    let mut reader = tokio::fs::read_dir(&canonical)
        .await
        .map_err(|_| ApiError::unavailable("The shared directory is unavailable."))?;
    while let Some(item) = reader
        .next_entry()
        .await
        .map_err(|_| ApiError::unavailable("The shared directory is unavailable."))?
    {
        let name = percent_encode_entry_name(&item.file_name());
        if !config.show_hidden && name.starts_with('.') {
            continue;
        }

        let item_path = item.path();
        let resolved = match tokio::fs::canonicalize(&item_path).await {
            Ok(value) => value,
            Err(_) => continue,
        };
        if ensure_within_root(&config.shared_root, &resolved).is_err() {
            continue;
        }

        let meta = match tokio::fs::metadata(&resolved).await {
            Ok(value) => value,
            Err(_) => continue,
        };

        let kind = classify_entry(&meta);
        let size_bytes = match kind {
            EntryKind::File => Some(meta.len()),
            EntryKind::Directory => None,
        };

        entries.push(DirectoryEntry {
            name,
            kind,
            size_bytes,
            modified_at: modified_to_iso(&meta),
        });
    }

    entries.sort_by(|left, right| match (&left.kind, &right.kind) {
        (EntryKind::Directory, EntryKind::File) => std::cmp::Ordering::Less,
        (EntryKind::File, EntryKind::Directory) => std::cmp::Ordering::Greater,
        _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    });

    Ok(Json(DirectoryListing {
        current_path: relative.clone(),
        parent_path: parent_path(&relative),
        entries,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/download",
    operation_id = "getApiV1Download",
    params(("p" = Option<String>, Query, description = "Relative file path under shared root")),
    responses(
        (status = 200, description = "File download stream"),
        (status = 206, description = "Partial content for a satisfiable Range request"),
        (status = 304, description = "Not modified; the If-None-Match validator matches"),
        (status = 400, description = "Invalid request", body = PublicErrorResponse),
        (status = 404, description = "File not found", body = PublicErrorResponse),
        (status = 416, description = "Range not satisfiable", body = PublicErrorResponse),
        (status = 503, description = "Storage unavailable", body = PublicErrorResponse)
    )
)]
async fn download(
    request_headers: HeaderMap,
    Query(query): Query<PathQuery>,
) -> Result<Response, ApiError> {
    let config = get_app_config()?;
    let relative = validate_relative_path(&query.p)?;
    if relative.is_empty() {
        return Err(ApiError::bad_request("The requested path is invalid."));
    }

    let canonical = canonical_entry_target(
        &config.shared_root,
        &relative,
        "The requested file could not be found.",
    )
    .await?;
    ensure_within_root(&config.shared_root, &canonical)?;

    // open_download_target verifies that the opened handle is a regular file whose
    // (dev, ino) matches the pre-open stat of the canonical target, blocking swaps
    // after the stat; the canonicalize-to-stat window remains (openat2 with
    // RESOLVE_IN_ROOT is the future full fix).
    let (mut file, meta) = open_download_target(&canonical).await?;

    let total = meta.len();
    let etag = compute_weak_etag(&meta);
    let etag_value = etag.as_ref().and_then(|value| value.to_str().ok());
    let modified = meta.modified().ok();

    // If-None-Match is evaluated before the Range header because a matching validator
    // means the client already holds the current representation.
    let none_match = request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok());
    if let Some(current_etag) = etag_value
        && none_match.is_some_and(|value| if_none_match_matches(value, current_etag))
    {
        let mut not_modified_headers = HeaderMap::new();
        if let Some(etag_header) = etag.clone() {
            not_modified_headers.insert(header::ETAG, etag_header);
        }
        return Ok((StatusCode::NOT_MODIFIED, not_modified_headers).into_response());
    }

    // A satisfiable range is only served when an If-Range validator matches. A
    // non-matching If-Range widens the request back to the full representation.
    let range_decision = request_headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(|value| parse_single_byte_range(value, total))
        .unwrap_or(RangeDecision::Full);
    let range_decision = match range_decision {
        RangeDecision::Partial(range) => {
            let if_range_honored = request_headers
                .get(header::IF_RANGE)
                .and_then(|value| value.to_str().ok())
                .map(|value| if_range_matches(value, etag_value, modified))
                .unwrap_or(true);
            if if_range_honored {
                RangeDecision::Partial(range)
            } else {
                RangeDecision::Full
            }
        }
        other => other,
    };

    let file_name = canonical
        .file_name()
        .unwrap_or(OsStr::new("download.bin"))
        .to_string_lossy()
        .to_string()
        .replace(['\r', '\n', '"', '\\'], "_");

    match range_decision {
        RangeDecision::NotSatisfiable => {
            let mut headers = HeaderMap::new();
            if let Ok(value) = header::HeaderValue::from_str(&format!("bytes */{total}")) {
                headers.insert(header::CONTENT_RANGE, value);
            }
            Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                headers,
                Json(PublicErrorResponse {
                    message: "The requested range is not satisfiable.".to_string(),
                }),
            )
                .into_response())
        }
        RangeDecision::Full => {
            let mut headers = content_response_headers(&file_name, etag.as_ref());
            headers.insert(header::CONTENT_LENGTH, header::HeaderValue::from(total));
            let body =
                Body::from_stream(ReaderStream::with_capacity(file, DOWNLOAD_STREAM_CAPACITY));
            Ok((StatusCode::OK, headers, body).into_response())
        }
        RangeDecision::Partial(range) => {
            file.seek(SeekFrom::Start(range.start))
                .await
                .map_err(|_| ApiError::unavailable("The shared directory is unavailable."))?;
            let mut headers = content_response_headers(&file_name, etag.as_ref());
            if let Ok(value) = header::HeaderValue::from_str(&format!(
                "bytes {}-{}/{}",
                range.start, range.end, total
            )) {
                headers.insert(header::CONTENT_RANGE, value);
            }
            headers.insert(
                header::CONTENT_LENGTH,
                header::HeaderValue::from(range.length()),
            );
            let body = Body::from_stream(ReaderStream::with_capacity(
                file.take(range.length()),
                DOWNLOAD_STREAM_CAPACITY,
            ));
            Ok((StatusCode::PARTIAL_CONTENT, headers, body).into_response())
        }
    }
}

fn embedded_asset_response(path: &str) -> Option<Response> {
    let file = FrontendAssets::get(path)?;
    let content_type = mime_guess::from_path(path).first_or_octet_stream();

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_str(content_type.as_ref())
            .unwrap_or_else(|_| header::HeaderValue::from_static("application/octet-stream")),
    );

    Some((StatusCode::OK, headers, file.data.into_owned()).into_response())
}

async fn frontend_asset(OriginalUri(uri): OriginalUri) -> Response {
    let request_path = uri.path().trim_start_matches('/');

    if request_path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path = if request_path.is_empty() {
        "index.html"
    } else {
        request_path
    };

    if let Some(response) = embedded_asset_response(path) {
        return response;
    }

    if let Some(response) = embedded_asset_response("index.html") {
        return response;
    }

    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Frontend assets are unavailable.",
    )
        .into_response()
}

pub fn app_router() -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/directory", get(directory))
        .route("/api/v1/download", get(download))
        .fallback(get(frontend_asset))
        .layer(TraceLayer::new_for_http())
}

#[derive(OpenApi)]
#[openapi(
    paths(health, directory, download),
    components(schemas(
        HealthResponse,
        PublicErrorResponse,
        EntryKind,
        DirectoryEntry,
        DirectoryListing
    ))
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use axum::http::StatusCode;
    use tempfile::TempDir;

    use super::{
        ByteRange, RangeDecision, compute_weak_etag, content_disposition_value,
        content_response_headers, ensure_within_root, etag_matches_weakly, if_none_match_matches,
        load_app_config, modified_to_iso, open_download_target, parse_config_file,
        parse_single_byte_range, percent_decode_relative_path, percent_encode_entry_name,
        percent_encode_ext_value, validate_relative_path,
    };

    #[test]
    fn missing_shared_root_names_the_directory_and_the_fix() {
        let temp = TempDir::new().expect("temp dir");
        let missing_root = temp.path().join("missing-share");
        let config_path = temp.path().join("config.yaml");
        fs::write(
            &config_path,
            format!("sharedRoot: {}\n", missing_root.display()),
        )
        .expect("write config");

        let error = load_app_config(&config_path).expect_err("missing sharedRoot should fail");

        assert!(error.contains("does not exist"));
        assert!(error.contains(missing_root.display().to_string().as_str()));
    }

    #[test]
    fn existing_shared_root_loads_with_defaults() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().join("share");
        fs::create_dir_all(&root).expect("create root");
        let config_path = temp.path().join("config.yaml");
        fs::write(&config_path, format!("sharedRoot: {}\n", root.display())).expect("write config");

        let config = load_app_config(&config_path).expect("load config");

        assert_eq!(config.listen_address, "0.0.0.0");
        assert_eq!(config.listen_port, 8080);
        assert!(!config.show_hidden);
        assert_eq!(
            config.shared_root,
            root.canonicalize().expect("canonical root")
        );
    }

    #[test]
    fn parse_config_file_dispatches_on_file_extension() {
        // A .json file is parsed as JSON; YAML content under a .json name fails.
        let parsed = parse_config_file(r#"{"sharedRoot": "/tmp/share"}"#, Path::new("config.json"))
            .expect("json config parses");
        assert_eq!(parsed.shared_root, "/tmp/share");
        assert_eq!(parsed.listen_port, None);

        let error = parse_config_file("sharedRoot: /tmp/share\n", Path::new("config.json"))
            .expect_err("yaml content under a json name should fail");
        assert_eq!(error, "Configuration file is invalid");

        // .yaml and .yml files are parsed as YAML.
        let parsed = parse_config_file(
            "sharedRoot: /tmp/share\nlistenPort: 9000\n",
            Path::new("config.yaml"),
        )
        .expect("yaml config parses");
        assert_eq!(parsed.listen_port, Some(9000));
        assert!(parse_config_file("sharedRoot: /tmp/share\n", Path::new("config.yml")).is_ok());

        // An unknown extension tries the YAML parser first. The JSON fallback arm
        // cannot be exercised through ConfigFile: serde_yaml accepts the JSON grammar
        // for this shape, and the derive rejects duplicate keys in both formats, so
        // every body one parser accepts the other accepts as well.
        let parsed = parse_config_file("sharedRoot: /tmp/share\n", Path::new("settings.conf"))
            .expect("unknown extension falls back to the yaml parse");
        assert_eq!(parsed.shared_root, "/tmp/share");

        // Garbage fails under every extension.
        let error = parse_config_file("not: [valid: config", Path::new("settings.conf"))
            .expect_err("garbage should fail");
        assert_eq!(error, "Configuration file is invalid");
    }

    #[test]
    fn load_app_config_rejects_blank_shared_root_and_blank_listen_address() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().join("share");
        fs::create_dir_all(&root).expect("create root");

        let blank_root = temp.path().join("blank-root.yaml");
        fs::write(&blank_root, "sharedRoot: \"   \"\n").expect("write config");
        let error = load_app_config(&blank_root).expect_err("blank sharedRoot should fail");
        assert_eq!(error, "Configuration sharedRoot is required");

        let blank_address = temp.path().join("blank-address.yaml");
        fs::write(
            &blank_address,
            format!("sharedRoot: {}\nlistenAddress: \"  \"\n", root.display()),
        )
        .expect("write config");
        let error = load_app_config(&blank_address).expect_err("blank listenAddress should fail");
        assert_eq!(
            error,
            "Configuration listenAddress must be a non-empty string"
        );
    }

    #[test]
    fn validate_relative_path_accepts_clean_relative_forms() {
        assert_eq!(validate_relative_path("").expect("empty path"), "");
        assert_eq!(validate_relative_path("   ").expect("whitespace path"), "");
        // A leading `.` component reaches the handler and is dropped; interior `.`
        // components are normalized away by the components iterator before it runs.
        assert_eq!(validate_relative_path(".").expect("dot path"), "");
        assert_eq!(
            validate_relative_path("./docs").expect("dot-prefixed path"),
            "docs"
        );
        assert_eq!(
            validate_relative_path("docs/./demo.txt").expect("interior dot path"),
            "docs/demo.txt"
        );
        // Trimming happens before processing.
        assert_eq!(
            validate_relative_path(" docs/demo.txt ").expect("trimmed path"),
            "docs/demo.txt"
        );
    }

    #[test]
    fn validate_relative_path_rejects_unsafe_and_traversal_forms() {
        // NUL bytes, backslashes and leading slashes are bad requests.
        for bad in ["bad\0name.txt", "back\\slash", "/etc/passwd", "//double"] {
            let error = validate_relative_path(bad).expect_err("unsafe path should be rejected");
            assert_eq!(error.status(), StatusCode::BAD_REQUEST, "for {bad:?}");
        }

        // A `..` component looks like a legitimate path but escapes the root, so it
        // is a not-found instead of a bad request.
        for traversal in ["..", "../secret", "a/../b"] {
            let error =
                validate_relative_path(traversal).expect_err("traversal should be rejected");
            assert_eq!(error.status(), StatusCode::NOT_FOUND, "for {traversal:?}");
        }
    }

    #[test]
    fn ensure_within_root_is_component_aware() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().join("root");
        let nested = root.join("nested").join("deep");
        // A sibling whose name extends the root's name must not pass:
        // Path::starts_with compares components, not raw strings, so /root-evil is
        // outside /root.
        let sibling = temp.path().join("root-evil");

        // The root itself and nested paths stay inside the root.
        assert!(ensure_within_root(&root, &root).is_ok());
        assert!(ensure_within_root(&root, &nested).is_ok());

        let error =
            ensure_within_root(&root, &sibling).expect_err("sibling prefix should be rejected");
        assert_eq!(error.status(), StatusCode::NOT_FOUND);

        // The parent of the root is outside as well.
        let error =
            ensure_within_root(&root, temp.path()).expect_err("root parent should be rejected");
        assert_eq!(error.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn percent_encode_entry_name_keeps_valid_utf8_and_encodes_invalid_bytes() {
        assert_eq!(
            percent_encode_entry_name(OsStr::new("demo.txt")),
            "demo.txt"
        );

        let raw = OsStr::from_bytes(b"bad\xffname.txt");
        assert_eq!(percent_encode_entry_name(raw), "bad%FFname.txt");
    }

    #[test]
    fn percent_decode_relative_path_round_trips_encoded_names() {
        let decoded =
            percent_decode_relative_path("docs/bad%FFname.txt").expect("decode encoded path");

        assert_eq!(
            decoded,
            PathBuf::from(OsStr::from_bytes(b"docs")).join(OsStr::from_bytes(b"bad\xffname.txt"))
        );
    }

    #[test]
    fn percent_encode_ext_value_escapes_per_rfc5987() {
        // attr-char passes through unescaped.
        assert_eq!(percent_encode_ext_value("a-b_1.~file"), "a-b_1.~file");
        // Space, percent, quote and the UTF-8 bytes of non-ASCII names are escaped.
        assert_eq!(percent_encode_ext_value("héllo.txt"), "h%C3%A9llo.txt");
        assert_eq!(percent_encode_ext_value("a b.txt"), "a%20b.txt");
        assert_eq!(percent_encode_ext_value("a%b.txt"), "a%25b.txt");
        assert_eq!(percent_encode_ext_value("a'b.txt"), "a%27b.txt");
    }

    #[test]
    fn content_disposition_keeps_plain_form_and_extends_non_ascii() {
        // ASCII names keep the plain quoted-string form.
        assert_eq!(
            content_disposition_value("demo.txt")
                .to_str()
                .expect("ascii name fits a header value"),
            "attachment; filename=\"demo.txt\""
        );
        // The caller sanitizes CR/LF/quote/backslash before this builder runs.
        assert_eq!(
            content_disposition_value("re_port.txt")
                .to_str()
                .expect("sanitized name fits a header value"),
            "attachment; filename=\"re_port.txt\""
        );

        // Non-ASCII names get the RFC 5987 companion: an ASCII-safe percent-encoded
        // fallback plus `filename*=UTF-8''...`.
        assert_eq!(
            content_disposition_value("héllo.txt")
                .to_str()
                .expect("encoded form is visible ascii"),
            "attachment; filename=\"h%C3%A9llo.txt\"; filename*=UTF-8''h%C3%A9llo.txt"
        );
    }

    #[test]
    fn content_response_headers_pin_download_media_type_and_range_support() {
        // Downloads always serve application/octet-stream regardless of the file
        // extension; nosniff makes clients honor it, and range support is advertised
        // on every 200/206 response.
        for name in ["demo.txt", "demo.html", "demo.unknownextension"] {
            let headers = content_response_headers(name, None);
            assert_eq!(
                headers
                    .get("content-type")
                    .and_then(|value| value.to_str().ok()),
                Some("application/octet-stream"),
                "for {name}"
            );
            assert_eq!(
                headers
                    .get("x-content-type-options")
                    .and_then(|value| value.to_str().ok()),
                Some("nosniff")
            );
            assert_eq!(
                headers
                    .get("accept-ranges")
                    .and_then(|value| value.to_str().ok()),
                Some("bytes")
            );
            assert!(headers.get("etag").is_none());
        }
    }

    #[test]
    fn modified_to_iso_reports_the_files_actual_time() {
        use chrono::{DateTime, Utc};
        use std::time::{Duration, SystemTime};

        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("mtime.bin");
        fs::write(&path, "content").expect("write file");
        let meta = fs::metadata(&path).expect("metadata");

        let iso = modified_to_iso(&meta).expect("a readable mtime yields Some");
        let parsed = DateTime::parse_from_rfc3339(&iso).expect("rfc3339 value");
        let mtime: DateTime<Utc> = meta.modified().expect("mtime").into();
        assert_eq!(parsed.with_timezone(&Utc), mtime);

        // A pre-epoch mtime is readable too, and it is reported as its actual value:
        // no `Utc::now()` substitution and no panic from the conversion. Filesystems
        // may reject the pre-epoch value at set time; where it is accepted, the actual
        // time must come back.
        let file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open file");
        let pre_epoch = SystemTime::UNIX_EPOCH
            .checked_sub(Duration::from_secs(10))
            .expect("pre-epoch time");
        file.set_times(fs::FileTimes::new().set_modified(pre_epoch))
            .expect("filesystem accepts a pre-epoch mtime");
        drop(file);

        let meta = fs::metadata(&path).expect("updated metadata");
        let iso = modified_to_iso(&meta).expect("pre-epoch mtime still yields Some");
        let parsed = DateTime::parse_from_rfc3339(&iso).expect("rfc3339 value");
        assert!(
            parsed.timestamp() < 0,
            "the actual pre-epoch time is reported, not a substituted value"
        );
    }

    #[tokio::test]
    async fn open_download_target_rejects_missing_and_non_file_targets() {
        let temp = TempDir::new().expect("temp dir");

        // A missing canonical target is rejected by the pre-open metadata step.
        let error = open_download_target(&temp.path().join("missing.bin"))
            .await
            .expect_err("missing target should fail");
        assert_eq!(error.status(), StatusCode::NOT_FOUND);

        // A directory target is rejected by the regular-file check on the opened
        // handle; the dev/ino comparison runs on that handle for every download.
        let dir = temp.path().join("dir");
        fs::create_dir_all(&dir).expect("create dir");
        let error = open_download_target(&dir)
            .await
            .expect_err("directory target should fail");
        assert_eq!(error.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn parse_single_byte_range_covers_open_ended_bounded_and_suffix_forms() {
        assert_eq!(
            parse_single_byte_range("bytes=2-", 10),
            RangeDecision::Partial(ByteRange { start: 2, end: 9 })
        );
        assert_eq!(
            parse_single_byte_range("bytes=0-4", 10),
            RangeDecision::Partial(ByteRange { start: 0, end: 4 })
        );
        assert_eq!(
            parse_single_byte_range("bytes=-3", 10),
            RangeDecision::Partial(ByteRange { start: 7, end: 9 })
        );
        // A suffix longer than the representation covers the whole file.
        assert_eq!(
            parse_single_byte_range("bytes=-99", 10),
            RangeDecision::Partial(ByteRange { start: 0, end: 9 })
        );
        // The end is clamped to the final byte of the representation.
        assert_eq!(
            parse_single_byte_range("bytes=0-100", 10),
            RangeDecision::Partial(ByteRange { start: 0, end: 9 })
        );
    }

    #[test]
    fn parse_single_byte_range_rejects_unsatisfiable_and_ignores_unusable() {
        assert_eq!(
            parse_single_byte_range("bytes=999999999-", 10),
            RangeDecision::NotSatisfiable
        );
        assert_eq!(
            parse_single_byte_range("bytes=10-", 10),
            RangeDecision::NotSatisfiable
        );
        assert_eq!(
            parse_single_byte_range("bytes=-0", 10),
            RangeDecision::NotSatisfiable
        );
        // An empty representation has no satisfiable range.
        assert_eq!(
            parse_single_byte_range("bytes=0-", 0),
            RangeDecision::NotSatisfiable
        );
        // Unsupported unit, malformed and multi-range specs are ignored.
        assert_eq!(
            parse_single_byte_range("items=0-4", 10),
            RangeDecision::Full
        );
        assert_eq!(
            parse_single_byte_range("bytes=abc", 10),
            RangeDecision::Full
        );
        assert_eq!(
            parse_single_byte_range("bytes=5-2", 10),
            RangeDecision::Full
        );
        assert_eq!(parse_single_byte_range("bytes=", 10), RangeDecision::Full);
        assert_eq!(
            parse_single_byte_range("bytes=0-4,10-20", 10),
            RangeDecision::Full
        );
    }

    #[test]
    fn etag_and_conditional_matching_is_weak_and_list_aware() {
        assert!(etag_matches_weakly("W/\"1a-5\"", "\"1a-5\""));
        assert!(etag_matches_weakly("\"1a-5\"", "W/\"1a-5\""));
        assert!(!etag_matches_weakly("W/\"1a-5\"", "\"2b-5\""));

        assert!(if_none_match_matches("*", "W/\"1a-5\""));
        assert!(if_none_match_matches("\"x\", W/\"1a-5\"", "W/\"1a-5\""));
        assert!(!if_none_match_matches("\"x\", \"y\"", "W/\"1a-5\""));
    }

    #[test]
    fn weak_etag_is_deterministic_per_file_state() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("etag.bin");
        fs::write(&path, "abcdef").expect("write file");
        let meta = fs::metadata(&path).expect("metadata");
        let etag = compute_weak_etag(&meta).expect("etag for a readable mtime");

        assert_eq!(compute_weak_etag(&meta), Some(etag.clone()));

        // A different length must produce a different validator.
        fs::write(&path, "abcdefghij").expect("rewrite file");
        let updated = fs::metadata(&path).expect("updated metadata");
        assert_ne!(compute_weak_etag(&updated), Some(etag));
    }

    #[test]
    fn percent_decode_relative_path_rejects_encoded_traversal() {
        // This is what axum's Query delivers for a client-encoded "%252F..%252F" escape;
        // decoding it back must not slip a ".." past validation.
        let error =
            percent_decode_relative_path("x%2F..%2Fy").expect_err("traversal should be rejected");

        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }
}
