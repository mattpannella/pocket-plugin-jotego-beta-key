// Pure logic (no extism), unit-tested natively. Ported from pupdate's CoresService.Jotego.cs,
// GithubApiService.DownloadFileContents and PatreonService.

use regex_lite::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;

pub const KEY_ZIP: &str = "jtbeta.zip";
pub const KEY_BIN: &str = "beta.bin";
pub const PATREON_VANITY: &str = "jotego";
pub const GITHUB_OWNER: &str = "JTFPGA";
pub const GITHUB_REPO: &str = "jtbeta";
pub const PAGE_SIZE: usize = 20;
pub const MAX_PAGES: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    // Aliases so a hand-edited config.json with "GitHub" etc. still loads.
    #[serde(alias = "GitHub", alias = "Github", alias = "GITHUB")]
    Github,
    #[serde(alias = "Patreon", alias = "PATREON")]
    Patreon,
    #[serde(alias = "Both", alias = "BOTH")]
    Both,
}

impl Source {
    pub const CHOICES: [(Source, &'static str); 3] = [
        (Source::Github, "GitHub (sponsor token with access to JTFPGA/jtbeta)"),
        (Source::Patreon, "Patreon (your session cookie)"),
        (Source::Both, "Both, GitHub first"),
    ];

    pub fn from_choice(value: &str) -> Option<Source> {
        Source::CHOICES.iter().find(|(_, label)| value.starts_with(label)).map(|(s, _)| *s)
    }

    pub fn uses_github(self) -> bool {
        self != Source::Patreon
    }

    pub fn uses_patreon(self) -> bool {
        self != Source::Github
    }
}

// Stored on the card at Plugins/jotego-beta-key/config.json
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patreon_session_id: Option<String>,
}

impl Config {
    pub fn parse(json: &str) -> Result<Config, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }
}

pub fn github_contents_url(api_base: &str) -> String {
    format!("{api_base}repos/{GITHUB_OWNER}/{GITHUB_REPO}/contents/{KEY_ZIP}")
}

pub fn patreon_page_url(base: &str) -> String {
    format!("{base}{PATREON_VANITY}")
}

pub fn patreon_posts_url(base: &str, campaign_id: &str) -> String {
    format!(
        "{base}api/posts?filter[campaign_id]={campaign_id}&filter[contains_exclusive_posts]=true&sort=-published_at\
         &include=attachments_media,attachments&fields[post]=title,url,published_at,current_user_can_view\
         &fields[media]=file_name,download_url&page[count]={PAGE_SIZE}"
    )
}

//the campaign id is embedded in the creator page's bootstrap JSON as "id":"123","type":"campaign".
pub fn campaign_id(html: &str) -> Option<String> {
    let re = Regex::new(r#""id"\s*:\s*"(\d+)"\s*,\s*"type"\s*:\s*"campaign""#).ok()?;
    re.captures(html).map(|c| c[1].to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub post_url: String,
    pub download_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    Found(Attachment),
    Gated(String),
    NotFound { next: Option<String> },
}

// One /api/posts page: the newest viewable post carrying `filename`, a gated one, or the next page.
pub fn probe_posts_page(json: &str, filename: &str) -> Result<Probe, String> {
    let doc: Value = serde_json::from_str(json).map_err(|e| format!("posts response is not JSON: {e}"))?;
    let empty = Vec::new();
    let included = doc.get("included").and_then(Value::as_array).unwrap_or(&empty);
    let media = |id: &str| {
        included.iter().find(|item| {
            item.get("type").and_then(Value::as_str) == Some("media") && item.get("id").and_then(Value::as_str) == Some(id)
        })
    };
    let mut gated: Option<String> = None;

    for post in doc.get("data").and_then(Value::as_array).unwrap_or(&empty) {
        //fall back to `attachments` when `attachments_media.data`
        // is missing or not an array (e.g. null).
        let rels = post
            .pointer("/relationships/attachments_media/data")
            .and_then(Value::as_array)
            .or_else(|| post.pointer("/relationships/attachments/data").and_then(Value::as_array))
            .unwrap_or(&empty);
        for rel in rels {
            let Some(item) = rel.get("id").and_then(Value::as_str).and_then(media) else { continue };
            let name = item.pointer("/attributes/file_name").and_then(Value::as_str).unwrap_or("");
            if !name.eq_ignore_ascii_case(filename) {
                continue;
            }
            let post_url = post.pointer("/attributes/url").and_then(Value::as_str).unwrap_or("(unknown post)").to_string();
            let can_view = post.pointer("/attributes/current_user_can_view").and_then(Value::as_bool).unwrap_or(false);
            if !can_view {
                gated.get_or_insert(post_url);
                continue;
            }
            let download_url = item
                .pointer("/attributes/download_url")
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())
                .ok_or_else(|| format!("found {filename} on post {post_url} but no download URL was returned"))?
                .to_string();
            return Ok(Probe::Found(Attachment { post_url, download_url }));
        }
    }

    if let Some(url) = gated {
        return Ok(Probe::Gated(url));
    }
    let next = doc.pointer("/links/next").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    Ok(Probe::NotFound { next })
}

pub const MAX_ENTRY_SIZE: u64 = 16 * 1024 * 1024;

pub fn extract_zip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a zip file: {e}"))?;
    let mut files = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("bad zip entry: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = safe_entry_path(entry.name()).ok_or_else(|| format!("unsafe path in the zip: {}", entry.name()))?;
        if name.split('/').any(|part| part.starts_with('.') || part == "__MACOSX") {
            continue;
        }
        let mut data = Vec::with_capacity(entry.size().min(MAX_ENTRY_SIZE) as usize);
        (&mut entry)
            .take(MAX_ENTRY_SIZE + 1)
            .read_to_end(&mut data)
            .map_err(|e| format!("could not read {name} from the zip: {e}"))?;
        if data.len() as u64 > MAX_ENTRY_SIZE {
            return Err(format!("{name} in the zip is larger than {} MiB", MAX_ENTRY_SIZE / 1024 / 1024));
        }
        files.push((name, data));
    }
    if files.is_empty() {
        return Err("the zip contains no files".into());
    }
    Ok(files)
}


fn safe_entry_path(raw: &str) -> Option<String> {
    let unified = raw.replace('\\', "/");
    if unified.starts_with('/') || unified.split('/').next().is_some_and(|first| first.contains(':')) {
        return None;
    }
    let mut parts = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn config_round_trips_and_rejects_garbage() {
        let c = Config { source: Some(Source::Both), github_token: Some("t".into()), patreon_session_id: None };
        let json = c.to_json();
        assert!(json.contains("\"both\"") && !json.contains("patreon_session_id"));
        assert_eq!(Config::parse(&json), Ok(c));
        assert!(Config::parse("nonsense").is_err());
        assert!(Config::parse(r#"{"source":"github",}"#).is_err());
        assert_eq!(Config::parse(r#"{"source":"GitHub"}"#).unwrap().source, Some(Source::Github));
        assert_eq!(Config::parse("{}"), Ok(Config::default()));
        assert_eq!(Source::from_choice("Patreon (your session cookie)"), Some(Source::Patreon));
        assert!(Source::Both.uses_github() && Source::Both.uses_patreon() && !Source::Github.uses_patreon());
    }

    #[test]
    fn campaign_id_is_scraped_from_bootstrap_json() {
        let html = r#"<html>...{"data":{"id":"1234567","type":"campaign","attributes":{}}}..."#;
        assert_eq!(campaign_id(html).as_deref(), Some("1234567"));
        assert_eq!(campaign_id("<html>nothing</html>"), None);
    }

    const PAGE: &str = r#"{
      "data": [
        { "attributes": { "url": "https://www.patreon.com/posts/old", "current_user_can_view": true },
          "relationships": { "attachments_media": { "data": [ { "id": "m1" } ] } } },
        { "attributes": { "url": "https://www.patreon.com/posts/gated", "current_user_can_view": false },
          "relationships": { "attachments_media": { "data": [ { "id": "m2" } ] } } },
        { "attributes": { "url": "https://www.patreon.com/posts/ok", "current_user_can_view": true },
          "relationships": { "attachments": { "data": [ { "id": "m3" } ] } } }
      ],
      "included": [
        { "type": "media", "id": "m1", "attributes": { "file_name": "notes.txt", "download_url": "u1" } },
        { "type": "media", "id": "m2", "attributes": { "file_name": "JTBETA.ZIP", "download_url": "u2" } },
        { "type": "media", "id": "m3", "attributes": { "file_name": "jtbeta.zip", "download_url": "https://c10.patreonusercontent.com/x/jtbeta.zip" } }
      ],
      "links": { "next": "https://www.patreon.com/api/posts?page=2" }
    }"#;

    #[test]
    fn posts_page_finds_newest_viewable_attachment() {
        let probe = probe_posts_page(PAGE, "jtbeta.zip").unwrap();
        assert_eq!(
            probe,
            Probe::Found(Attachment {
                post_url: "https://www.patreon.com/posts/ok".into(),
                download_url: "https://c10.patreonusercontent.com/x/jtbeta.zip".into()
            })
        );
    }

    #[test]
    fn posts_page_reports_gated_or_next() {
        let gated_only = PAGE.replace(r#""url": "https://www.patreon.com/posts/ok", "current_user_can_view": true"#,
            r#""url": "https://www.patreon.com/posts/ok", "current_user_can_view": false"#);
        assert_eq!(probe_posts_page(&gated_only, "jtbeta.zip").unwrap(), Probe::Gated("https://www.patreon.com/posts/gated".into()));
        assert_eq!(
            probe_posts_page(PAGE, "other.zip").unwrap(),
            Probe::NotFound { next: Some("https://www.patreon.com/api/posts?page=2".into()) }
        );
        assert_eq!(probe_posts_page(r#"{"data":[]}"#, "jtbeta.zip").unwrap(), Probe::NotFound { next: None });
        assert!(probe_posts_page("<html>", "jtbeta.zip").is_err());
    }

    #[test]
    fn null_attachments_media_falls_back_to_attachments() {
        let page = r#"{
          "data": [ { "attributes": { "url": "p", "current_user_can_view": true },
                      "relationships": { "attachments_media": { "data": null }, "attachments": { "data": [ { "id": "m1" } ] } } } ],
          "included": [ { "type": "media", "id": "m1", "attributes": { "file_name": "jtbeta.zip", "download_url": "u" } } ]
        }"#;
        assert_eq!(
            probe_posts_page(page, "jtbeta.zip").unwrap(),
            Probe::Found(Attachment { post_url: "p".into(), download_url: "u".into() })
        );
    }

    #[test]
    fn zip_entries_keep_their_paths() {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            w.add_directory("sub/", opts).unwrap();
            w.start_file("sub/beta.bin", opts).unwrap();
            w.write_all(b"KEYBYTES").unwrap();
            w.start_file("readme.txt", opts).unwrap();
            w.write_all(b"hi").unwrap();
            w.start_file("__MACOSX/._beta.bin", opts).unwrap();
            w.write_all(b"junk").unwrap();
            w.start_file("win\\beta.bin", opts).unwrap();
            w.write_all(b"W").unwrap();
            w.finish().unwrap();
        }
        let files = extract_zip(&buf.into_inner()).unwrap();
        assert_eq!(
            files,
            vec![
                ("sub/beta.bin".to_string(), b"KEYBYTES".to_vec()),
                ("readme.txt".to_string(), b"hi".to_vec()),
                ("win/beta.bin".to_string(), b"W".to_vec()),
            ]
        );
        assert!(extract_zip(b"not a zip").is_err());
    }

    #[test]
    fn oversized_entries_are_rejected() {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            w.start_file("beta.bin", opts).unwrap();
            w.write_all(&vec![0u8; (MAX_ENTRY_SIZE + 1) as usize]).unwrap();
            w.finish().unwrap();
        }
        let err = extract_zip(&buf.into_inner()).unwrap_err();
        assert!(err.contains("larger than"), "{err}");
    }

    #[test]
    fn unsafe_zip_paths_are_rejected() {
        assert_eq!(safe_entry_path("a/./b.bin").as_deref(), Some("a/b.bin"));
        assert_eq!(safe_entry_path("../evil"), None);
        assert_eq!(safe_entry_path("a\\..\\..\\evil"), None);
        assert_eq!(safe_entry_path("/etc/passwd"), None);
        assert_eq!(safe_entry_path("C:\\x"), None);
    }
}
