use crate::config::AppConfig;
use crate::fetch::api;
use crate::identity::ServerIdentity;

fn parse_version_string(version_str: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let components: Vec<&str> = version_str.split('.').collect();

    if components.len() < 2 || components.len() > 3 {
        return Err("ERROR: Incorrectly formatted version string. Expected format: X.Y or X.Y.Z".into());
    }

    let multipliers = [1_000_000, 10_000, 100];
    let mut final_version = 0;

    for (index, component) in components.iter().enumerate() {
        if component.len() > 3 {
            return Err("ERROR: Incorrectly formatted version string. Components cannot exceed 3 characters.".into());
        }

        let numeric_value: i32 = component
            .parse()
            .map_err(|_| "ERROR: Incorrectly formatted version string. Components must be numbers.")?;

        final_version += numeric_value * multipliers[index];
    }

    Ok(final_version)
}

pub async fn execute_fetch(version_input: &str, show_ui: bool) -> Result<(), Box<dyn std::error::Error>> {
    let identity = ServerIdentity::load();
    let config = AppConfig::load();

    let target_version = parse_version_string(version_input)?;

    if show_ui {
        println!("Initiating fetch target for validated version code: {}", target_version);
    }

    api::download_target(&identity, &config, target_version, show_ui).await?;

    Ok(())
}