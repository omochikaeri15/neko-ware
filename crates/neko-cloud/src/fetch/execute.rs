use crate::config::AppConfig;
use crate::fetch::{game, update, download};
use crate::identity::ServerIdentity;

pub async fn execute_fetch(
    version_input: Option<&str>,
    region_input: &str,
    game_path_input: Option<&str>,
    show_ui: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let identity = ServerIdentity::load();
    let config = AppConfig::load();

    let region_profile = update::parse_region_string(region_input)?;

    let (target_version, payload_targets) = match game_path_input {
        Some(path) => {
            let extracted = game::extract_payload_from_binary(path, region_input)?;
            (0, Some(extracted))
        }
        None => {
            let ver_str = version_input.ok_or("ERROR: Missing target version. Provide either '--update' or '--game'.")?;
            let parsed_ver = update::parse_version_string(ver_str)?;
            (parsed_ver, None)
        }
    };

    download::execute_download_pipeline(
        &identity,
        &config,
        target_version,
        &region_profile.project_name,
        &region_profile.lang_suffix,
        payload_targets,
        show_ui,
    )
        .await?;

    Ok(())
}