use reqwest::{header, Client};

pub fn format_url(
    base_url: &str,
    project_name: &str,
    lang_suffix: &str,
    version: i32,
    index: i32,
) -> String {
    let string_code = format_string_code(project_name, version, index);

    format!(
        "{}/iphone/{}/download/{}{}.zip",
        base_url, project_name, string_code, lang_suffix
    )
}

pub fn format_string_code(project_name: &str, version: i32, index: i32) -> String {
    if version < 1_000_000 {
        return format!("{}_{}_{}", project_name, version, index);
    }

    format!(
        "{}_{:06}_{:02}_{:02}",
        project_name,
        version / 100,
        index,
        version % 100
    )
}

pub fn build_client(signed_cookie: &str) -> Result<Client, Box<dyn std::error::Error>> {
    let mut headers = header::HeaderMap::new();
    headers.insert(header::ACCEPT_ENCODING, header::HeaderValue::from_static("gzip"));
    headers.insert(header::CONNECTION, header::HeaderValue::from_static("keep-alive"));
    headers.insert(header::RANGE, header::HeaderValue::from_static("bytes=0-"));
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static("Dalvik/2.1.0 (Linux; U; Android 9; Pixel 2 Build/PQ3A.190801.002)"),
    );

    let cookie_value = header::HeaderValue::from_str(signed_cookie)?;
    headers.insert(header::COOKIE, cookie_value);

    let client = Client::builder().default_headers(headers).build()?;
    Ok(client)
}