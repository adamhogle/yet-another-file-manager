use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use axum::extract::{OriginalUri, Query};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use once_cell::sync::OnceCell;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

#[derive(Embed)]
#[folder = "../frontend/dist/"]
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
    pub modified_at: String,
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
        "json" => serde_json::from_str(raw).map_err(|_| "Configuration file is invalid".to_string()),
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

    let canonical_root = fs::canonicalize(&parsed.shared_root)
        .map_err(|_| "Configuration sharedRoot is invalid".to_string())?;
    if !canonical_root.is_dir() {
        return Err("Configuration sharedRoot must be a directory".to_string());
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
    CONFIG.get_or_try_init(|| load_app_config(config_path)).map(|_| ())
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

fn modified_to_iso(meta: &fs::Metadata) -> String {
    let modified = meta.modified().ok();
    let dt: DateTime<Utc> = modified.map(DateTime::<Utc>::from).unwrap_or_else(Utc::now);
    dt.to_rfc3339()
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
    let candidate = config.shared_root.join(&relative);

    let canonical = fs::canonicalize(&candidate).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            ApiError::not_found("The requested directory could not be found.")
        } else {
            ApiError::unavailable("The shared directory is unavailable.")
        }
    })?;

    ensure_within_root(&config.shared_root, &canonical)?;

    if !canonical.is_dir() {
        return Err(ApiError::not_found(
            "The requested directory could not be found.",
        ));
    }

    let mut entries: Vec<DirectoryEntry> = Vec::new();
    for item in fs::read_dir(&canonical)
        .map_err(|_| ApiError::unavailable("The shared directory is unavailable."))?
    {
        let item = match item {
            Ok(value) => value,
            Err(_) => continue,
        };

        let name = item.file_name().to_string_lossy().to_string();
        if !config.show_hidden && name.starts_with('.') {
            continue;
        }

        let item_path = item.path();
        let resolved = match fs::canonicalize(&item_path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if ensure_within_root(&config.shared_root, &resolved).is_err() {
            continue;
        }

        let meta = match fs::metadata(&resolved) {
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
        (status = 400, description = "Invalid request", body = PublicErrorResponse),
        (status = 404, description = "File not found", body = PublicErrorResponse),
        (status = 503, description = "Storage unavailable", body = PublicErrorResponse)
    )
)]
async fn download(Query(query): Query<PathQuery>) -> Result<Response, ApiError> {
    let config = get_app_config()?;
    let relative = validate_relative_path(&query.p)?;
    if relative.is_empty() {
        return Err(ApiError::bad_request("The requested path is invalid."));
    }

    let candidate = config.shared_root.join(&relative);
    let canonical = fs::canonicalize(&candidate).map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            ApiError::not_found("The requested file could not be found.")
        } else {
            ApiError::unavailable("The shared directory is unavailable.")
        }
    })?;
    ensure_within_root(&config.shared_root, &canonical)?;

    let meta = fs::metadata(&canonical)
        .map_err(|_| ApiError::unavailable("The shared directory is unavailable."))?;
    if !meta.is_file() {
        return Err(ApiError::not_found(
            "The requested file could not be found.",
        ));
    }

    let bytes = fs::read(&canonical)
        .map_err(|_| ApiError::unavailable("The shared directory is unavailable."))?;

    let file_name = canonical
        .file_name()
        .unwrap_or(OsStr::new("download.bin"))
        .to_string_lossy()
        .to_string()
        .replace(['\r', '\n', '"', '\\'], "_");

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{file_name}\""))
            .unwrap_or_else(|_| header::HeaderValue::from_static("attachment")),
    );
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        header::HeaderValue::from_static("nosniff"),
    );

    Ok((StatusCode::OK, headers, bytes).into_response())
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
