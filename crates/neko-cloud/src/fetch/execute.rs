use crate::config::AppConfig;
use crate::fetch::{payload, download};
use crate::identity::ServerIdentity;
use std::path::Path;

pub async fn execute_fetch(
    input: &str,
    region_input: &str,
    show_ui: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let identity = ServerIdentity::load();
    let config = AppConfig::load();

    let region_profile = payload::parse_region_string(region_input)?;

    let input_path = Path::new(input);
    let (target_payload, payload_targets) = if input_path.exists() {
        let extracted = payload::extract_payload_from_binary(input, region_input)?;
        (0, Some(extracted))
    } else {
        let parsed_payload = input.parse::<i32>().map_err(|_| "ERROR: Input must be a valid file path or a numeric payload timestamp string (e.g., '15040000').")?;
        (parsed_payload, None)
    };

    download::execute_download_pipeline(
        &identity,
        &config,
        target_payload,
        &region_profile.project_name,
        &region_profile.lang_suffix,
        payload_targets,
        show_ui,
    )
        .await?;

    Ok(())
}