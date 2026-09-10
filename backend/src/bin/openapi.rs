use std::env;
use std::fs;
use std::path::Path;

use backend::ApiDoc;
use utoipa::openapi::info::License;
use utoipa::OpenApi;

fn main() {
    let output_path = env::args()
        .nth(1)
        .unwrap_or_else(|| "../api/openapi.yaml".to_string());

    let mut openapi = ApiDoc::openapi();
    openapi.info.title = "Yet Another File Manager".to_string();
    openapi.info.description = Some(
        "A web-based file manager for self-hosted local file sharing.".to_string(),
    );
    let mut license = License::new("AGPL-3.0-only");
    license.url = Some("https://www.gnu.org/licenses/agpl-3.0.txt".to_string());
    openapi.info.license = Some(license);

    let yaml = serde_yaml::to_string(&openapi).expect("failed to serialize OpenAPI to YAML");

    if let Some(parent) = Path::new(&output_path).parent() {
        fs::create_dir_all(parent).expect("failed to create OpenAPI output directory");
    }

    fs::write(&output_path, yaml).expect("failed to write OpenAPI file");
    println!("Wrote OpenAPI spec to {output_path}");
}
