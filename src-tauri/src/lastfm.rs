use serde::{Deserialize, Serialize};

const FALLBACK_API_KEY: &str = "565ef10e832779c021eb06c5a415f3a5";
const FALLBACK_SHARED_SECRET: &str = "0123456789abcdef0123456789abcdef";
const BASE_URL: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrobbleConfig {
    pub username: String,
    pub session_key: String,
    pub enabled: bool,
    pub api_key: String,
    pub api_secret: String,
}

impl ScrobbleConfig {
    pub fn effective_api_key(&self) -> &str {
        if self.api_key.is_empty() { FALLBACK_API_KEY } else { &self.api_key }
    }
    pub fn effective_api_secret(&self) -> &str {
        if self.api_secret.is_empty() { FALLBACK_SHARED_SECRET } else { &self.api_secret }
    }
}

pub fn get_auth_url(api_key: &str) -> String {
    let key = if api_key.is_empty() { FALLBACK_API_KEY } else { api_key };
    format!("https://www.last.fm/api/auth/?api_key={key}")
}

pub async fn get_session(
    token: &str,
    config: &ScrobbleConfig,
    client: &reqwest::Client,
) -> Result<(String, String), String> {
    let api_key = config.effective_api_key();
    let params = [
        ("method", "auth.getSession"),
        ("token", token),
        ("api_key", api_key),
    ];
    let secret = config.effective_api_secret();
    let signature = sign(&params, secret);
    let mut form: Vec<(&str, &str)> = params.to_vec();
    form.push(("api_sig", &signature));
    form.push(("format", "json"));

    let resp = client
        .post(BASE_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("Last.fm request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        log::warn!("Last.fm API error: {body}");
        return Err(format!("Last.fm API error: {body}"));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse session response: {e}"))?;
    let session = json["session"]
        .as_object()
        .ok_or_else(|| "No session in response".to_string())?;
    let name = session["name"]
        .as_str()
        .ok_or_else(|| "No username in session".to_string())?
        .to_string();
    let key = session["key"]
        .as_str()
        .ok_or_else(|| "No session key in response".to_string())?
        .to_string();
    Ok((name, key))
}

pub async fn now_playing(
    config: &ScrobbleConfig,
    artist: &str,
    track: &str,
    album: &str,
    duration: u32,
    client: &reqwest::Client,
) -> Result<(), String> {
    if !config.enabled || config.session_key.is_empty() {
        return Ok(());
    }
    let api_key = config.effective_api_key();
    let params = [
        ("method", "track.updateNowPlaying"),
        ("artist", artist),
        ("track", track),
        ("album", album),
        ("duration", &duration.to_string()),
        ("api_key", api_key),
        ("sk", &config.session_key),
    ];
    let secret = config.effective_api_secret();
    send_request(client, &params, secret).await
}

pub async fn scrobble(
    config: &ScrobbleConfig,
    artist: &str,
    track: &str,
    album: &str,
    timestamp: i64,
    client: &reqwest::Client,
) -> Result<(), String> {
    if !config.enabled || config.session_key.is_empty() {
        return Ok(());
    }
    let api_key = config.effective_api_key();
    let params = [
        ("method", "track.scrobble"),
        ("artist", artist),
        ("track", track),
        ("album", album),
        ("timestamp", &timestamp.to_string()),
        ("api_key", api_key),
        ("sk", &config.session_key),
    ];
    let secret = config.effective_api_secret();
    send_request(client, &params, secret).await
}

fn sign(params: &[(&str, &str)], secret: &str) -> String {
    let mut sorted: Vec<(&str, &str)> = params.to_vec();
    sorted.sort_by_key(|(k, _)| *k);
    let concat: String = sorted
        .iter()
        .map(|(k, v)| format!("{}{}", k, v))
        .collect::<Vec<_>>()
        .join("");
    let to_sign = format!("{}{}", concat, secret);
    use md5::{Md5, Digest};
    let digest = Md5::digest(to_sign.as_bytes());
    format!("{:x}", digest)
}

async fn send_request_raw(
    client: &reqwest::Client,
    params: &[(&str, &str)],
    secret: &str,
) -> Result<reqwest::Response, String> {
    let signature = sign(params, secret);

    let mut form = Vec::new();
    for (k, v) in params {
        form.push((*k, *v));
    }
    form.push(("api_sig", signature.as_str()));
    form.push(("format", "json"));

    let resp = client
        .post(BASE_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("Last.fm request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        log::warn!("Last.fm API error: {body}");
        return Err(format!("Last.fm API error: {body}"));
    }

    Ok(resp)
}

async fn send_request(
    client: &reqwest::Client,
    params: &[(&str, &str)],
    secret: &str,
) -> Result<(), String> {
    send_request_raw(client, params, secret).await?;
    Ok(())
}

pub async fn track_get_top_tags(
    api_key: &str,
    artist: &str,
    track: &str,
    client: &reqwest::Client,
) -> Result<Vec<String>, String> {
    let key = if api_key.is_empty() { FALLBACK_API_KEY } else { api_key };
    let params = [
        ("method", "track.getTopTags"),
        ("artist", artist),
        ("track", track),
        ("api_key", key),
    ];
    let body = send_request_raw_unsigned(client, &params).await?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("Failed to parse top tags: {e}"))?;
    let mut tags = Vec::new();
    if let Some(toptags) = json["toptags"]["tag"].as_array() {
        for tag in toptags {
            if let Some(name) = tag["name"].as_str() {
                tags.push(name.to_string());
            }
        }
    }
    Ok(tags)
}

pub async fn artist_get_top_tags(
    api_key: &str,
    artist: &str,
    client: &reqwest::Client,
) -> Result<Vec<String>, String> {
    let key = if api_key.is_empty() { FALLBACK_API_KEY } else { api_key };
    let params = [
        ("method", "artist.getTopTags"),
        ("artist", artist),
        ("api_key", key),
    ];
    let body = send_request_raw_unsigned(client, &params).await?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("Failed to parse top tags: {e}"))?;
    let mut tags = Vec::new();
    if let Some(toptags) = json["toptags"]["tag"].as_array() {
        for tag in toptags {
            if let Some(name) = tag["name"].as_str() {
                tags.push(name.to_string());
            }
        }
    }
    Ok(tags)
}

pub async fn validate_api_key(api_key: &str, client: &reqwest::Client) -> Result<String, String> {
    let key = if api_key.is_empty() { FALLBACK_API_KEY } else { api_key };
    let params = [
        ("method", "artist.getTopTags"),
        ("artist", "Radiohead"),
        ("api_key", key),
    ];
    let body = send_request_raw_unsigned(client, &params).await?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|_| "Failed to parse response".to_string())?;

    if let Some(error) = json["error"].as_i64() {
        if error == 10 || error == 6 {
            return Err("Invalid API key — check your Last.fm API key".to_string());
        }
        return Err(format!("Last.fm error {error}"));
    }

    if json["toptags"].is_object() {
        Ok("Key valid ✓".to_string())
    } else {
        Err("Unexpected response from Last.fm".to_string())
    }
}

async fn send_request_raw_unsigned(
    client: &reqwest::Client,
    params: &[(&str, &str)],
) -> Result<String, String> {
    let mut form = Vec::new();
    for (k, v) in params {
        form.push((*k, *v));
    }
    form.push(("format", "json"));

    let resp = client
        .post(BASE_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("Last.fm request failed: {e}"))?;

    let is_success = resp.status().is_success();
    let body = resp.text().await.unwrap_or_default();
    if !is_success && !body.contains("\"error\":6") {
        log::warn!("Last.fm API error: {body}");
        return Err(format!("Last.fm API error: {body}"));
    }

    Ok(body)
}
