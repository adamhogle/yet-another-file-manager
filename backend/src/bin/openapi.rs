use std::env;
use std::fs;
use std::path::Path;

use backend::ApiDoc;
use utoipa::OpenApi;

fn main() {
    let output_path = env::args()
        .nth(1)
        .unwrap_or_else(|| "../api/openapi.yaml".to_string());

    let openapi = ApiDoc::openapi();
    let json_value = serde_json::to_value(&openapi).expect("failed to serialize OpenAPI");
    let yaml = serde_yaml::to_string(&json_value).expect("failed to render OpenAPI yaml");

    if let Some(parent) = Path::new(&output_path).parent() {
        fs::create_dir_all(parent).expect("failed to create OpenAPI output directory");
    }

    fs::write(&output_path, yaml).expect("failed to write OpenAPI file");
    println!("Wrote OpenAPI spec to {output_path}");
}
