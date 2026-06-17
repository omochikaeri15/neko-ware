use crate::config::AppConfig;
use crate::fetch::api::{build_client, format_url};
use crate::fetch::cloudfront::generate_signed_cookie;
use crate::identity::ServerIdentity;
use crate::io::get_local_dir;
use colored::Colorize;
use futures::{stream, StreamExt};
use reqwest::Client;
use std::fs;
use std::fs::File;
use std::io::Write;
use tracing::{debug, info};

async fn scan_for_valid_urls(
    client: &Client,
    base_url: &str,
    project_name: &str,
    lang_suffix: &str,
    target_version: i32,
    start_index: i32,
    end_index: i32,
    show_ui: bool,
) -> Vec<String> {
    let mut valid_urls = Vec::new();

    let mut scanner_stream = stream::iter(start_index..=end_index)
        .map(|index| {
            let url = format_url(base_url, project_name, lang_suffix, target_version, index);
            async move {
                debug!(url = %url, "Sending HEAD request");
                let response = client.head(&url).send().await;
                (index, url, response)
            }
        })
        .buffer_unordered(20);

    while let Some((index, url, result)) = scanner_stream.next().await {
        let response = match result {
            Ok(server_response) => server_response,
            Err(error) => {
                debug!(index, error = %error, "Network error during HEAD request");
                continue;
            }
        };

        if !response.status().is_success() {
            debug!(index, status = %response.status(), "File not present in this version");
            continue;
        }

        if show_ui {
            println!("    {} Found versions target payload at Index {:02}", "✓".green(), index);
        }

        info!(index, version = target_version, "Valid update payload found");
        valid_urls.push(url);
    }

    valid_urls
}

async fn download_valid_urls(
    client: &Client,
    config: &AppConfig,
    valid_urls: Vec<String>,
    show_ui: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut output_dir = get_local_dir();
    output_dir.push(&config.output_dir);
    fs::create_dir_all(&output_dir)?;

    if show_ui {
        println!("\n  {} Downloading found payload(s)...", "⟳".cyan());
    }

    for url in valid_urls {
        let file_name = url.split('/').last().unwrap_or("downloaded_file.zip");
        let mut file_path = output_dir.clone();
        file_path.push(file_name);

        let response = client.get(&url).send().await?;
        let byte_stream = response.bytes().await?;

        let mut file = File::create(&file_path)?;
        file.write_all(&byte_stream)?;

        if show_ui {
            println!("    {} Downloaded {}", "✓".green(), file_name);
        }

        info!(file = %file_path.display(), "Successfully wrote update payload to disk");
    }

    if show_ui {
        println!("\nSUCCESS: Downloaded payloads from server!\n");
    }

    Ok(())
}

pub async fn execute_download_pipeline(
    identity: &ServerIdentity,
    config: &AppConfig,
    target_version: i32,
    project_name: &str,
    lang_suffix: &str,
    payload_targets: Option<Vec<i32>>,
    show_ui: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_url = identity
        .ponos_server_url
        .as_deref()
        .ok_or("ERROR: PONOS Server URL is missing. Run 'neko-cloud identity create'.")?;

    let resource_url = format!("{}/*", base_url);
    let signed_cookie = generate_signed_cookie(identity, &resource_url)?;
    let client = build_client(&signed_cookie)?;

    if show_ui {
        println!();

        let trigger_warning = match &payload_targets {
            Some(targets) => targets.iter().any(|&v| v >= 14_070_000),
            None => target_version >= 14_070_000,
        };

        if trigger_warning {
            let orange_excl = "!".truecolor(255, 165, 0);
            println!("{} WARNING, PLEASE READ CAREFULLY:", orange_excl);
            println!("{} Game devs have switched to a new update serving system for version 14.7+", orange_excl);
            println!("{} Newer updates may have to be obtained from the games {} split", orange_excl, "InstallPack.apk".cyan());
            println!("{} Expect web requests for newer versions to fail or yield incorrect/old assets\n", orange_excl);
        }

        if payload_targets.is_some() {
            println!("  {} Scanning code for payloads to download from server...", "⟳".cyan());
        } else {
            println!("  {} Scanning server for targeted versions payloads...", "⟳".cyan());
        }
    }

    let valid_urls = match payload_targets {
        Some(targets) => {
            let mut discovered_urls = Vec::new();

            let mut check_stream = stream::iter(targets.into_iter().enumerate())
                .map(|(index, version)| {
                    let idx = index as i32;
                    let url = format_url(base_url, project_name, lang_suffix, version, idx);

                    let active_client = client.clone();

                    async move {
                        debug!(url = %url, "Checking binary-extracted payload target via HEAD");
                        let response = active_client.head(&url).send().await;
                        (idx, version, url, response)
                    }
                })
                .buffer_unordered(20);

            while let Some((index, version, url, result)) = check_stream.next().await {
                if let Ok(response) = result {
                    if response.status().is_success() {
                        if show_ui {
                            println!("    {} Found explicit target payload {:>8} at Index {:02}", "✓".green(), version, index);
                        }
                        discovered_urls.push(url);
                    }
                }
            }
            discovered_urls
        }
        None => {
            let range_string = config.search_index_range.as_deref().unwrap_or("0-50");
            let range_parts: Vec<&str> = range_string.split('-').collect();
            let start_index = range_parts.first().unwrap_or(&"0").parse::<i32>().unwrap_or(0);
            let end_index = range_parts.get(1).unwrap_or(&"50").parse::<i32>().unwrap_or(50);

            scan_for_valid_urls(
                &client,
                base_url,
                project_name,
                lang_suffix,
                target_version,
                start_index,
                end_index,
                show_ui,
            )
                .await
        }
    };

    if valid_urls.is_empty() {
        if show_ui {
            println!("\nFAILURE: Found no payloads for targeted version!\n");
        }
        return Ok(());
    }

    download_valid_urls(&client, config, valid_urls, show_ui).await?;

    Ok(())
}