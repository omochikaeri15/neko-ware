pub struct RegionProfile {
    pub project_name: String,
    pub lang_suffix: String,
}

pub fn parse_version_string(version_str: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let components: Vec<&str> = version_str.split('.').collect();

    if components.len() == 1 {
        let numeric_value: i32 = components[0]
            .parse()
            .map_err(|_| "ERROR: Incorrectly formatted version string. Components must be numbers.")?;
        return Ok(numeric_value);
    }

    if components.len() > 3 {
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

pub fn parse_region_string(region_str: &str) -> Result<RegionProfile, Box<dyn std::error::Error>> {
    let components: Vec<&str> = region_str.split('_').collect();

    if components.is_empty() || components.len() > 2 {
        return Err("ERROR: Incorrectly formatted region string. Expected format: XX or XX_YY.".into());
    }

    let base_region = components[0].to_lowercase();
    if base_region.len() != 2 {
        return Err("ERROR: Region base must be exactly 2 letters (e.g., 'en', 'jp').".into());
    }

    let sub_language = if components.len() == 2 {
        let suffix = components[1].to_lowercase();
        if suffix.len() != 2 {
            return Err("ERROR: Region sub-language must be exactly 2 letters (e.g., 'it', 'fr').".into());
        }
        format!("_{}", suffix)
    } else {
        String::new()
    };

    let project_name = match base_region.as_str() {
        "jp" | "ja" => "battlecats",
        "en" => "battlecatsen",
        "kr" | "ko" => "battlecatskr",
        "tw" => "battlecatstw",
        _ => return Err(format!("ERROR: Unsupported region '{}'. Supported: en, jp/ja, kr/ko, tw.", base_region).into()),
    };

    Ok(RegionProfile {
        project_name: String::from(project_name),
        lang_suffix: sub_language,
    })
}