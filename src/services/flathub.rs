//! Flathub's catalogue, for the Shop: browse and search apps, read one app's
//! page, and download their icons and screenshots.
//!
//! Read-only: installing goes through the host's Flatpak
//! (`platform::flatpak_host`), or the system's software store ([`store_uri`])
//! without it. Only apps built for this machine's architecture are listed.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use serde::Deserialize;
use thiserror::Error;

const API: &str = "https://flathub.org/api/v2";
/// Where Flathub serves icons and screenshots. Nothing else is downloaded.
const MEDIA_HOSTS: &[&str] = &["dl.flathub.org"];
/// Apps per request.
pub const PAGE_SIZE: usize = 48;
/// A picture larger than this is not downloaded.
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum FlathubError {
    #[error("could not reach Flathub: {0}")]
    Network(String),
    #[error("Flathub's answer was not understood: {0}")]
    Unreadable(String),
    #[error("not a Flathub app ID")]
    InvalidAppId,
}

/// What the Shop lists: Flathub's main categories, games first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Games,
    All,
    AudioVideo,
    Development,
    Education,
    Graphics,
    Internet,
    Office,
    Science,
    System,
    Utilities,
    /// What's installed on this machine (not a Flathub category).
    Installed,
}

impl Category {
    pub const ALL: [Self; 12] = [
        Self::Games,
        Self::All,
        Self::AudioVideo,
        Self::Development,
        Self::Education,
        Self::Graphics,
        Self::Internet,
        Self::Office,
        Self::Science,
        Self::System,
        Self::Utilities,
        Self::Installed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Games => "Games",
            Self::All => "All apps",
            Self::AudioVideo => "Audio & Video",
            Self::Development => "Development",
            Self::Education => "Education",
            Self::Graphics => "Graphics",
            Self::Internet => "Internet",
            Self::Office => "Office",
            Self::Science => "Science",
            Self::System => "System",
            Self::Utilities => "Utilities",
            Self::Installed => "Installed",
        }
    }

    /// Flathub's name for it (`main_categories`); `None` for every app.
    fn filter(self) -> Option<&'static str> {
        Some(match self {
            Self::Games => "Game",
            Self::All | Self::Installed => return None,
            Self::AudioVideo => "AudioVideo",
            Self::Development => "Development",
            Self::Education => "Education",
            Self::Graphics => "Graphics",
            Self::Internet => "Network",
            Self::Office => "Office",
            Self::Science => "Science",
            Self::System => "System",
            Self::Utilities => "Utility",
        })
    }
}

/// An app as the catalogue lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSummary {
    pub app_id: String,
    pub name: String,
    pub summary: String,
    pub developer: String,
    pub icon_url: Option<String>,
    /// Flathub has checked the developer is who they say.
    pub verified: bool,
}

/// One page of a listing, and how many apps the whole listing has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub apps: Vec<AppSummary>,
    pub total: usize,
}

/// One app's page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDetails {
    pub app_id: String,
    /// Paragraphs, with list items as "• " lines.
    pub description: String,
    /// The screenshots, at about 1248 px wide.
    pub screenshots: Vec<String>,
}

/// The software store's link to an app: it opens on the app, ready to
/// install.
pub fn store_uri(app_id: &str) -> Result<String, FlathubError> {
    valid_app_id(app_id)?;
    Ok(format!("appstream://{app_id}"))
}

/// The app's Flathub web page, for a system with no software store.
pub fn web_uri(app_id: &str) -> Result<String, FlathubError> {
    valid_app_id(app_id)?;
    Ok(format!("https://flathub.org/apps/{app_id}"))
}

/// A Flathub app ID: reverse-DNS, letters, digits, `_`, `-` and dots only,
/// starting with a letter (so it can never read as a command-line option).
pub fn valid_app_id(app_id: &str) -> Result<(), FlathubError> {
    if crate::sources::flatpak::valid_app_id(app_id) {
        Ok(())
    } else {
        Err(FlathubError::InvalidAppId)
    }
}

/// Flathub's name for this machine's architecture.
fn arch() -> &'static str {
    std::env::consts::ARCH
}

/// One connection pool for every request: a blocking client runs its own
/// thread, and a kept connection skips the TLS handshake.
static SHARED: Mutex<Option<FlathubClient>> = Mutex::new(None);

#[derive(Clone)]
pub struct FlathubClient {
    client: reqwest::blocking::Client,
}

impl FlathubClient {
    /// The client every request shares, made on first use.
    pub fn shared() -> Result<Self, FlathubError> {
        let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(client) = shared.as_ref() {
            return Ok(client.clone());
        }
        let client = Self::new()?;
        *shared = Some(client.clone());
        Ok(client)
    }

    fn new() -> Result<Self, FlathubError> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("Horizon/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| FlathubError::Network(error.to_string()))?;
        Ok(Self { client })
    }

    /// A page (from 1) of `category`, most installed first, or of what
    /// matches `query` when it isn't empty (every category).
    pub fn list(&self, category: Category, query: &str, page: usize) -> Result<Page, FlathubError> {
        let query = query.trim();
        let mut filters = vec![serde_json::json!({ "filterType": "arches", "value": arch() })];
        if query.is_empty()
            && let Some(category) = category.filter()
        {
            filters.push(serde_json::json!({ "filterType": "main_categories", "value": category }));
        }
        let body = serde_json::json!({
            "query": query,
            "filters": filters,
            "hits_per_page": PAGE_SIZE,
            "page": page.max(1),
        });
        let text = self
            .client
            .post(format!("{API}/search"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::text)
            .map_err(|error| FlathubError::Network(error.without_url().to_string()))?;
        parse_page(&text)
    }

    pub fn details(&self, app_id: &str) -> Result<AppDetails, FlathubError> {
        valid_app_id(app_id)?;
        let text = self
            .client
            .get(format!("{API}/appstream/{app_id}"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::text)
            .map_err(|error| FlathubError::Network(error.without_url().to_string()))?;
        parse_details(app_id, &text)
    }

    /// One app as the catalogue would list it, from its page.
    pub fn app(&self, app_id: &str) -> Result<AppSummary, FlathubError> {
        valid_app_id(app_id)?;
        let text = self
            .client
            .get(format!("{API}/appstream/{app_id}"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::text)
            .map_err(|error| FlathubError::Network(error.without_url().to_string()))?;
        parse_app(app_id, &text)
    }

    /// An icon or screenshot from Flathub's media host, over HTTPS. An
    /// allowlisted image transport, not a generic URL fetcher.
    pub fn media(&self, url: &str) -> Option<Vec<u8>> {
        if !is_media_url(url) {
            return None;
        }
        let response = self.client.get(url).send().ok()?.error_for_status().ok()?;
        if response
            .content_length()
            .is_some_and(|len| len > MAX_IMAGE_BYTES)
        {
            return None;
        }
        let bytes = response.bytes().ok()?;
        (bytes.len() as u64 <= MAX_IMAGE_BYTES).then(|| bytes.to_vec())
    }
}

fn is_media_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|address| {
        address.scheme() == "https"
            && address
                .host_str()
                .is_some_and(|host| MEDIA_HOSTS.contains(&host))
            && address.port().is_none()
            && address.username().is_empty()
            && address.password().is_none()
    })
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    hits: Vec<Hit>,
    #[serde(rename = "totalHits", default)]
    total_hits: usize,
}

#[derive(Deserialize)]
struct Hit {
    app_id: String,
    name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    developer_name: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    verification_verified: Option<bool>,
}

fn parse_page(text: &str) -> Result<Page, FlathubError> {
    let response: SearchResponse =
        serde_json::from_str(text).map_err(|error| FlathubError::Unreadable(error.to_string()))?;
    let apps = response
        .hits
        .into_iter()
        .filter(|hit| valid_app_id(&hit.app_id).is_ok())
        .map(|hit| AppSummary {
            name: clean_line(&hit.name),
            summary: clean_line(hit.summary.as_deref().unwrap_or_default()),
            developer: clean_line(hit.developer_name.as_deref().unwrap_or_default()),
            icon_url: hit.icon.filter(|url| is_media_url(url)),
            verified: hit.verification_verified.unwrap_or(false),
            app_id: hit.app_id,
        })
        .collect();
    Ok(Page {
        apps,
        total: response.total_hits,
    })
}

#[derive(Deserialize)]
struct AppstreamSummary {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    developer_name: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    metadata: Option<HashMap<String, serde_json::Value>>,
}

fn parse_app(app_id: &str, text: &str) -> Result<AppSummary, FlathubError> {
    let found: AppstreamSummary =
        serde_json::from_str(text).map_err(|error| FlathubError::Unreadable(error.to_string()))?;
    let verified = found
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("flathub::verification::verified"))
        .is_some_and(|value| value == "true" || value == true);
    Ok(AppSummary {
        app_id: app_id.to_owned(),
        name: clean_line(found.name.as_deref().unwrap_or(app_id)),
        summary: clean_line(found.summary.as_deref().unwrap_or_default()),
        developer: clean_line(found.developer_name.as_deref().unwrap_or_default()),
        icon_url: found.icon.filter(|url| is_media_url(url)),
        verified,
    })
}

#[derive(Deserialize)]
struct Appstream {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    screenshots: Vec<Screenshot>,
}

#[derive(Deserialize)]
struct Screenshot {
    #[serde(default)]
    sizes: Vec<ScreenshotSize>,
}

#[derive(Deserialize)]
struct ScreenshotSize {
    width: String,
    src: String,
}

/// Screenshots are shown about this wide.
const SCREENSHOT_WIDTH: u32 = 1248;

fn parse_details(app_id: &str, text: &str) -> Result<AppDetails, FlathubError> {
    let appstream: Appstream =
        serde_json::from_str(text).map_err(|error| FlathubError::Unreadable(error.to_string()))?;
    let screenshots = appstream
        .screenshots
        .into_iter()
        .filter_map(|shot| {
            let sizes: Vec<(u32, String)> = shot
                .sizes
                .into_iter()
                .filter(|size| is_media_url(&size.src))
                .filter_map(|size| Some((size.width.parse().ok()?, size.src)))
                .collect();
            // The smallest at least as wide as shown, else the widest.
            sizes
                .iter()
                .filter(|(width, _)| *width >= SCREENSHOT_WIDTH)
                .min_by_key(|(width, _)| *width)
                .or_else(|| sizes.iter().max_by_key(|(width, _)| *width))
                .map(|(_, src)| src.clone())
        })
        .collect();
    Ok(AppDetails {
        app_id: app_id.to_owned(),
        description: plain_text(appstream.description.as_deref().unwrap_or_default()),
        screenshots,
    })
}

/// One line of text: whitespace runs (newlines too) become one space.
fn clean_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// AppStream's description markup (`<p>`, `<ul>`/`<ol>` with `<li>`, `<em>`,
/// `<code>`) as plain paragraphs: list items become "• " lines, and
/// anything else is dropped to its text.
fn plain_text(markup: &str) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut rest = markup;
    let flush = |current: &mut String, blocks: &mut Vec<String>| {
        let line = clean_line(&decode_entities(current));
        if !line.is_empty() {
            blocks.push(line);
        }
        current.clear();
    };
    while let Some(open) = rest.find('<') {
        current.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].trim().to_ascii_lowercase();
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default();
        match name {
            "p" | "ul" | "ol" | "br" => flush(&mut current, &mut blocks),
            "li" => {
                flush(&mut current, &mut blocks);
                if !tag.starts_with('/') {
                    current.push_str("• ");
                }
            }
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    current.push_str(rest);
    flush(&mut current, &mut blocks);
    // A list item stays with the lines around it; paragraphs are spaced.
    let mut text = String::new();
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            let in_list = block.starts_with("• ") && blocks[index - 1].starts_with("• ");
            text.push_str(if in_list { "\n" } else { "\n\n" });
        }
        text.push_str(block);
    }
    text
}

fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_are_checked_before_they_become_links() {
        assert_eq!(
            store_uri("org.supertuxproject.SuperTux").unwrap(),
            "appstream://org.supertuxproject.SuperTux"
        );
        assert_eq!(
            web_uri("com.valvesoftware.Steam").unwrap(),
            "https://flathub.org/apps/com.valvesoftware.Steam"
        );
        for bad in [
            "",
            "steam",
            "org.example",
            "org..example.App",
            "org.example.App/../x",
            "org.example.App?x=1",
        ] {
            assert!(store_uri(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_flathub_media_is_downloaded() {
        assert!(is_media_url(
            "https://dl.flathub.org/media/icons/128x128/a.b.C.png"
        ));
        assert!(!is_media_url("http://dl.flathub.org/media/a.png"));
        assert!(!is_media_url("https://example.com/a.png"));
        assert!(!is_media_url("https://dl.flathub.org:8443/a.png"));
        assert!(!is_media_url("https://user@dl.flathub.org/a.png"));
    }

    #[test]
    fn a_page_keeps_valid_apps_and_its_total() {
        let page = parse_page(
            r#"{"hits":[
                {"app_id":"org.supertuxproject.SuperTux","name":"SuperTux","summary":"A jump-and-run\n game",
                 "developer_name":"The SuperTux Team","icon":"https://dl.flathub.org/media/icons/st.png",
                 "verification_verified":true},
                {"app_id":"bad id","name":"Nope"},
                {"app_id":"org.example.NoIcon","name":"No Icon","icon":"https://example.com/x.png"}
            ],"totalHits":604}"#,
        )
        .unwrap();
        assert_eq!(page.total, 604);
        assert_eq!(page.apps.len(), 2);
        assert_eq!(page.apps[0].summary, "A jump-and-run game");
        assert!(page.apps[0].verified);
        assert_eq!(page.apps[1].icon_url, None);
        assert!(!page.apps[1].verified);
        assert!(parse_page("not json").is_err());
    }

    #[test]
    fn details_pick_screenshots_near_the_shown_size() {
        let details = parse_details(
            "org.example.App",
            r#"{"description":"<p>One &amp; two.</p><ul><li>First</li><li>Second</li></ul><p>End</p>",
                "screenshots":[
                  {"sizes":[{"width":"1600","src":"https://dl.flathub.org/a_orig.png"},
                            {"width":"1248","src":"https://dl.flathub.org/a_1248.png"},
                            {"width":"624","src":"https://dl.flathub.org/a_624.png"}]},
                  {"sizes":[{"width":"800","src":"https://dl.flathub.org/b_800.png"},
                            {"width":"400","src":"https://dl.flathub.org/b_400.png"}]},
                  {"sizes":[{"width":"1248","src":"https://example.com/c.png"}]}
                ]}"#,
        )
        .unwrap();
        assert_eq!(
            details.screenshots,
            vec![
                "https://dl.flathub.org/a_1248.png".to_owned(),
                "https://dl.flathub.org/b_800.png".to_owned()
            ]
        );
        assert_eq!(
            details.description,
            "One & two.\n\n• First\n• Second\n\nEnd"
        );
    }

    #[test]
    fn descriptions_without_markup_or_with_stray_tags_stay_readable() {
        assert_eq!(plain_text("Just text"), "Just text");
        assert_eq!(
            plain_text("<p>Use <code>--help</code> or <em>not</em></p>"),
            "Use --help or not"
        );
        assert_eq!(plain_text("<p>Broken <b"), "Broken");
        assert_eq!(plain_text(""), "");
    }

    #[test]
    fn an_apps_own_page_lists_it() {
        let app = parse_app(
            "org.example.Game",
            r#"{"name":"Game","summary":"Fun","developer_name":"Dev",
                "icon":"https://dl.flathub.org/media/icon.png",
                "metadata":{"flathub::verification::verified":"true"}}"#,
        )
        .unwrap();
        assert_eq!(app.name, "Game");
        assert!(app.verified);
        let bare = parse_app("org.example.Bare", "{}").unwrap();
        assert_eq!(bare.name, "org.example.Bare");
        assert!(!bare.verified);
    }

    #[test]
    fn every_category_but_all_and_installed_filters() {
        for category in Category::ALL {
            assert_eq!(
                category.filter().is_none(),
                matches!(category, Category::All | Category::Installed)
            );
            assert!(!category.label().is_empty());
        }
    }
}
