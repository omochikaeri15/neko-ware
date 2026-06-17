use crate::config::AppConfig;
use crate::fetch::cloudfront::generate_signed_cookie;
use crate::identity::ServerIdentity;
use crate::io::get_local_dir;
use colored::Colorize;
use futures::{stream, StreamExt};
use reqwest::{header, Client};
use std::fs;
use std::fs::File;
use std::io::Write;
use tracing::{debug, info};

fn format_url(base_url: &str, version: i32, index: i32) -> String {
    if version < 1_000_000 {
        format!(
            "{}/iphone/battlecatsen/download/battlecatsen_{}_{}.zip",
            base_url, version, index
        )
    } else {
        format!(
            "{}/iphone/battlecatsen/download/battlecatsen_{:06}_{:02}_{:02}.zip",
            base_url, version / 100, index, version % 100
        )
    }
}

pub async fn download_target(
    identity: &ServerIdentity,
    config: &AppConfig,
    target_version: i32,
    show_ui: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_url = identity
        .ponos_server_url
        .as_deref()
        .ok_or("ERROR: PONOS Server URL is missing. Run 'neko-cloud identity create'.")?;

    let resource_url = format!("{}/*", base_url);
    let signed_cookie = generate_signed_cookie(identity, &resource_url)?;

    let mut headers = header::HeaderMap::new();
    headers.insert(header::ACCEPT_ENCODING, header::HeaderValue::from_static("gzip"));
    headers.insert(header::CONNECTION, header::HeaderValue::from_static("keep-alive"));
    headers.insert(header::RANGE, header::HeaderValue::from_static("bytes=0-"));
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static("Dalvik/2.1.0 (Linux; U; Android 9; Pixel 2 Build/PQ3A.190801.002)"),
    );

    let cookie_value = header::HeaderValue::from_str(&signed_cookie)?;
    headers.insert(header::COOKIE, cookie_value);

    let client = Client::builder().default_headers(headers).build()?;

    // Parse the range safely, defaulting to 0-350 if malformed or missing
    let range_str = config.search_index_range.as_deref().unwrap_or("0-350");
    let parts: Vec<&str> = range_str.split('-').collect();
    let start_index = parts.first().unwrap_or(&"0").parse::<i32>().unwrap_or(0);
    let end_index = parts.get(1).unwrap_or(&"350").parse::<i32>().unwrap_or(350);

    let mut valid_urls = Vec::new();

    if show_ui {
        println!(
            "{} Scanning CloudFront (Indexes {}-{}) for new files in version {}...",
            "⟳".cyan(), start_index, end_index, target_version
        );
    }

    let mut scanner_stream = stream::iter(start_index..=end_index)
        .map(|index| {
            let active_client = &client;
            let url = format_url(base_url, target_version, index);
            async move {
                debug!(url = %url, "Sending HEAD request");
                let response = active_client.head(&url).send().await;
                (index, url, response)
            }
        })
        .buffer_unordered(20);

    while let Some((index, url, result)) = scanner_stream.next().await {
        match result {
            Ok(response) if response.status().is_success() => {
                if show_ui {
                    println!("  {} Found new asset payload at Index {:03}!", "✓".green(), index);
                }
                info!(index, version = target_version, "Valid update payload found");
                valid_urls.push(url);
            }
            Ok(response) => {
                debug!(index, status = %response.status(), "File not present in this version");
            }
            Err(error) => {
                debug!(index, error = %error, "Network error during HEAD request");
            }
        }
    }

    if valid_urls.is_empty() {
        if show_ui {
            println!("\n{} No new files found for version {}. The server might not be updated yet.", "ℹ".yellow(), target_version);
        }
        return Ok(());
    }

    let mut output_dir = get_local_dir();
    output_dir.push(&config.output_dir);
    fs::create_dir_all(&output_dir)?;

    if show_ui {
        println!("\n{} Downloading {} new payload(s)...", "↓".cyan(), valid_urls.len());
    }

    for url in valid_urls {
        let file_name = url.split('/').last().unwrap_or("downloaded_file.zip");
        let mut file_path = output_dir.clone();
        file_path.push(file_name);

        let response = client.get(&url).send().await?;
        let bytes = response.bytes().await?;

        let mut file = File::create(&file_path)?;
        file.write_all(&bytes)?;

        if show_ui {
            println!("  {} Saved {}", "✓".green(), file_name);
        }
        info!(file = %file_path.display(), "Successfully wrote update payload to disk");
    }

    if show_ui {
        println!("\n{} Update fetch complete! Files are in {}", "★".green(), output_dir.display());
    }

    Ok(())
}