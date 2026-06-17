use crate::identity::ServerIdentity;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rsa::{pkcs1::DecodeRsaPrivateKey, pkcs8::DecodePrivateKey, Pkcs1v15Sign, RsaPrivateKey};
use sha1::{Digest, Sha1};
use time::OffsetDateTime;
use tracing::debug;

pub fn generate_signed_cookie(
    identity: &ServerIdentity,
    resource_url: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let policy = make_policy(resource_url);
    let signature_bytes = generate_signature(identity, &policy)?;

    let safe_policy = BASE64.encode(policy.as_bytes())
        .replace('+', "-")
        .replace('=', "_")
        .replace('/', "~");

    let safe_sig = BASE64.encode(signature_bytes)
        .replace('+', "-")
        .replace('=', "_")
        .replace('/', "~");

    let key_pair_id = identity
        .key_pair_id
        .as_deref()
        .ok_or("ERROR: CloudFront Key Pair ID is null. Run 'neko-cloud identity create'.")?;

    Ok(format!(
        "CloudFront-Key-Pair-Id={}; CloudFront-Policy={}; CloudFront-Signature={}",
        key_pair_id, safe_policy, safe_sig
    ))
}

fn make_policy(url: &str) -> String {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let date_less_than = now + 3600;
    let date_greater_than = now - 3600;

    format!(
        r#"{{"Statement":[{{"Resource":"{}","Condition":{{"DateLessThan":{{"AWS:EpochTime":{}}},"DateGreaterThan":{{"AWS:EpochTime":{}}}}}}}]}}"#,
        url, date_less_than, date_greater_than
    )
}

fn generate_signature(
    identity: &ServerIdentity,
    policy: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let private_key_string = identity
        .rsa_private_key
        .as_deref()
        .ok_or("ERROR: RSA Private Key is null. Run 'neko-cloud identity create'.")?;

    let private_key = if private_key_string.contains("BEGIN RSA PRIVATE KEY") {
        RsaPrivateKey::from_pkcs1_pem(private_key_string)
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    } else {
        RsaPrivateKey::from_pkcs8_pem(private_key_string)
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }?;

    let mut hasher = Sha1::new();
    hasher.update(policy.as_bytes());
    let hashed_policy = hasher.finalize();

    let signing_key = Pkcs1v15Sign::new::<Sha1>();
    let signature_bytes = private_key.sign(signing_key, &hashed_policy)?;

    debug!("Successfully generated RSA SHA1 signature for CloudFront policy");
    Ok(signature_bytes)
}