//! SteamGridDB API v2 metadata and conservative game resolution.
//!
//! This is an on-demand, blocking metadata/image client. Never call it from Slint's
//! UI thread. CDN downloads use a separate unauthenticated HTTP request.
use std::{
    io::{Cursor, Read},
    thread,
    time::Duration,
};

use image::{ImageFormat, RgbaImage};
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::Deserialize;
use thiserror::Error;

use crate::{
    domain::LibraryGame,
    sources::{ExternalArtworkId, SourceRegistry},
};

const API_BASE: &str = "https://www.steamgriddb.com/api/v2/";
const RESPONSE_LIMIT: u64 = 1024 * 1024;
const IMAGE_LIMIT: u64 = 12 * 1024 * 1024;
const RETRY_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Error)]
pub enum SteamGridDbError {
    #[error("Could not initialize the SteamGridDB HTTPS client")]
    Initialize(#[source] reqwest::Error),
    #[error("SteamGridDB connection failed or timed out")]
    Network(#[source] reqwest::Error),
    #[error("SteamGridDB API key was rejected")]
    Unauthorized,
    #[error("SteamGridDB rate limit reached")]
    RateLimited,
    #[error("SteamGridDB service is temporarily unavailable (HTTP {0})")]
    Unavailable(u16),
    #[error("SteamGridDB returned an unexpected HTTP status ({0})")]
    Http(u16),
    #[error("SteamGridDB returned invalid metadata")]
    InvalidResponse,
    #[error("SteamGridDB metadata response exceeded the size limit")]
    OversizedResponse,
    #[error("SteamGridDB artwork download failed (HTTP {0})")]
    ImageHttp(u16),
    #[error("SteamGridDB artwork was invalid or not a supported native square")]
    InvalidImage,
    #[error("SteamGridDB artwork exceeded the download limit")]
    OversizedImage,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SteamGridDbGame {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamGridDbSquareGrid {
    pub id: u64,
    pub image_url: Url,
    pub author: Option<String>,
    // Official API response ranking signal. None ranks after scored entries.
    pub score: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMethod {
    ExactPlatformId,
    UniqueExactTitle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamGridDbMatch {
    pub game: SteamGridDbGame,
    pub method: MatchMethod,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchOutcome {
    Matched(SteamGridDbMatch),
    NotFound,
    // The user must resolve ambiguity; never silently select the first hit.
    Ambiguous,
}

#[derive(Deserialize)]
struct ApiResponse<T> {
    success: bool,
    data: T,
}

// SteamGridDB search responses may return a direct game object, while some
// documented examples wrap individual hits in an additional `data` object.
// Accept either, but still apply the conservative unique-exact-title filter.
#[derive(Deserialize)]
#[serde(untagged)]
enum ApiSearchHit {
    Direct(SteamGridDbGame),
    Wrapped { data: SteamGridDbGame },
}

#[derive(Deserialize)]
struct ApiGrid {
    id: u64,
    url: String,
    #[serde(default)]
    score: Option<i64>,
    #[serde(default)]
    author: Option<ApiAuthor>,
}

#[derive(Deserialize)]
struct ApiAuthor {
    name: String,
}

/// Rank all safe metadata candidates by SteamGridDB vote score before any
/// download. Missing scores are ranked after scored entries; ties keep the
/// API's original order. A downloaded image's resolution never outranks score.
fn sorted_eligible_artwork(grids: Vec<ApiGrid>) -> Vec<SteamGridDbSquareGrid> {
    let mut eligible = grids
        .into_iter()
        .filter_map(|grid| {
            let image_url = Url::parse(&grid.url).ok()?;
            if !is_steamgriddb_https(&image_url) {
                return None;
            }
            Some(SteamGridDbSquareGrid {
                id: grid.id,
                image_url,
                author: grid.author.map(|author| author.name),
                score: grid.score,
            })
        })
        .collect::<Vec<_>>();
    eligible.sort_by_key(|grid| std::cmp::Reverse(grid.score));
    eligible
}

/// Requests are limited in time and size. No API key is included in a URL,
/// error message or log output. Redirects are disabled to avoid credential
/// forwarding to an unexpected endpoint.
pub struct SteamGridDbClient {
    http: Client,
    base: Url,
    api_key: String,
}

impl SteamGridDbClient {
    pub fn new(api_key: &str) -> Result<Self, SteamGridDbError> {
        let http = Client::builder()
            .user_agent("Horizon/9.5.43 (SteamGridDB artwork)")
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(10))
            .redirect(Policy::none())
            .build()
            .map_err(SteamGridDbError::Initialize)?;
        Ok(Self {
            http,
            base: Url::parse(API_BASE).expect("constant SteamGridDB API URL is valid"),
            api_key: api_key.to_owned(),
        })
    }

    /// Exact platform mapping; a 404 is a normal lack of metadata, not a
    /// reason to fall back to guessing a Steam title by name.
    pub fn game_by_external_id(
        &self,
        external: &ExternalArtworkId,
    ) -> Result<Option<SteamGridDbGame>, SteamGridDbError> {
        let (platform, id) = match external {
            ExternalArtworkId::SteamAppId(id) => ("steam", id.to_string()),
        };
        let url = self.api_url(&["games", platform, &id]);
        self.request_json::<SteamGridDbGame>(url)
    }

    /// Name search is used as a conservative fallback only. Results must
    /// contain a unique exact title match; fuzzy guesses are never accepted.
    pub fn search_by_name(&self, name: &str) -> Result<Vec<SteamGridDbGame>, SteamGridDbError> {
        let url = self.api_url(&["search", "autocomplete", name]);
        Ok(self
            .request_json::<Vec<ApiSearchHit>>(url)?
            .unwrap_or_default()
            .into_iter()
            .map(|hit| match hit {
                ApiSearchHit::Direct(game) | ApiSearchHit::Wrapped { data: game } => game,
            })
            .collect())
    }

    /// Retrieves only static native 1:1 grid candidates.
    pub fn square_grids(
        &self,
        game_id: u64,
    ) -> Result<Vec<SteamGridDbSquareGrid>, SteamGridDbError> {
        let mut url = self.api_url(&["grids", "game", &game_id.to_string()]);
        url.query_pairs_mut()
            .append_pair("dimensions", "512x512,1024x1024")
            .append_pair("mimes", "image/png,image/jpeg")
            .append_pair("types", "static")
            .append_pair("nsfw", "false")
            .append_pair("humor", "false")
            .append_pair("epilepsy", "false")
            .append_pair("limit", "50")
            .append_pair("page", "0");

        let grids = self.request_json::<Vec<ApiGrid>>(url)?.unwrap_or_default();
        Ok(sorted_eligible_artwork(grids))
    }

    /// Icons are a last resort when a game has no native-square grid.
    /// Accept square PNG sources >=256px, preferring the largest decoded source.
    pub fn square_icons(
        &self,
        game_id: u64,
    ) -> Result<Vec<SteamGridDbSquareGrid>, SteamGridDbError> {
        let mut url = self.api_url(&["icons", "game", &game_id.to_string()]);
        url.query_pairs_mut()
            .append_pair("dimensions", "256,512,768,1024")
            .append_pair("mimes", "image/png")
            .append_pair("types", "static")
            .append_pair("nsfw", "false")
            .append_pair("humor", "false")
            .append_pair("epilepsy", "false")
            .append_pair("limit", "50")
            .append_pair("page", "0");
        let icons = self.request_json::<Vec<ApiGrid>>(url)?.unwrap_or_default();
        Ok(sorted_eligible_artwork(icons))
    }

    /// Downloads CDN art without the Bearer token. Redirects are disabled,
    /// final URLs have already passed the HTTPS host allowlist, and image bytes
    /// plus decoded dimensions are strictly bounded.
    pub fn download_square_grid(
        &self,
        grid: &SteamGridDbSquareGrid,
    ) -> Result<RgbaImage, SteamGridDbError> {
        if !is_steamgriddb_https(&grid.image_url) {
            return Err(SteamGridDbError::InvalidImage);
        }
        let response = self
            .http
            .get(grid.image_url.clone())
            .send()
            .map_err(SteamGridDbError::Network)?;
        if response.status().as_u16() != 200 {
            return Err(SteamGridDbError::ImageHttp(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|len| len > IMAGE_LIMIT)
        {
            return Err(SteamGridDbError::OversizedImage);
        }
        let mut bytes = Vec::new();
        response
            .take(IMAGE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| SteamGridDbError::InvalidImage)?;
        if bytes.len() as u64 > IMAGE_LIMIT {
            return Err(SteamGridDbError::OversizedImage);
        }
        decode_square_grid(&bytes)
    }

    fn api_url(&self, parts: &[&str]) -> Url {
        let mut url = self.base.clone();
        // API_BASE ends in `/`. Without removing its empty final segment,
        // `extend` produces `/api/v2//games/...`, which the API can return as
        // 404; that 404 was incorrectly interpreted as an unmatched game.
        url.path_segments_mut()
            .expect("API base has hierarchical path")
            .pop_if_empty()
            .extend(parts.iter().copied());
        url
    }

    fn request_json<T: for<'de> Deserialize<'de>>(
        &self,
        url: Url,
    ) -> Result<Option<T>, SteamGridDbError> {
        // Retry one transient server failure only, not 401, 404 or 429.
        for attempt in 0..2 {
            let response = self
                .http
                .get(url.clone())
                .bearer_auth(&self.api_key)
                .send()
                .map_err(SteamGridDbError::Network)?;
            let status = response.status().as_u16();
            match status {
                404 => return Ok(None),
                401 | 403 => return Err(SteamGridDbError::Unauthorized),
                429 => return Err(SteamGridDbError::RateLimited),
                502..=504 if attempt == 0 => {
                    thread::sleep(RETRY_DELAY);
                    continue;
                }
                500..=599 => return Err(SteamGridDbError::Unavailable(status)),
                200 => {}
                _ => return Err(SteamGridDbError::Http(status)),
            }
            let mut body = Vec::new();
            response
                .take(RESPONSE_LIMIT + 1)
                .read_to_end(&mut body)
                .map_err(|_| SteamGridDbError::InvalidResponse)?;
            if body.len() as u64 > RESPONSE_LIMIT {
                return Err(SteamGridDbError::OversizedResponse);
            }
            let parsed: ApiResponse<T> =
                serde_json::from_slice(&body).map_err(|_| SteamGridDbError::InvalidResponse)?;
            if !parsed.success {
                return Err(SteamGridDbError::InvalidResponse);
            }
            return Ok(Some(parsed.data));
        }
        unreachable!("transient retries exit through a request result")
    }
}

fn decode_square_grid(bytes: &[u8]) -> Result<RgbaImage, SteamGridDbError> {
    let format = image::guess_format(bytes).map_err(|_| SteamGridDbError::InvalidImage)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(SteamGridDbError::InvalidImage);
    }
    let (w, h) = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| SteamGridDbError::InvalidImage)?;
    if w != h || !matches!(w, 256 | 512 | 768 | 1024) {
        return Err(SteamGridDbError::InvalidImage);
    }
    image::load_from_memory_with_format(bytes, format)
        .map(image::DynamicImage::into_rgba8)
        .map_err(|_| SteamGridDbError::InvalidImage)
}

fn is_steamgriddb_https(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url
            .host_str()
            .is_some_and(|host| host == "steamgriddb.com" || host.ends_with(".steamgriddb.com"))
}

/// Gather source-authoritative IDs on the application thread before dispatching
/// a worker. The owned lookup plan is Send-safe: no Rc/Slint/source adapters
/// accompany blocking network work to the worker thread.
#[derive(Debug, Clone)]
pub struct SteamGridDbLookup {
    title: String,
    external_ids: Vec<ExternalArtworkId>,
}

impl SteamGridDbLookup {
    pub fn for_game(game: &LibraryGame, sources: &SourceRegistry) -> Self {
        let external_ids = game
            .sources()
            .iter()
            .filter_map(|source_ref| {
                sources
                    .get(source_ref.source_id())
                    .and_then(|source| source.external_artwork_id(source_ref.external_id()))
            })
            .collect::<Vec<_>>();
        Self {
            title: game.game().title().as_str().to_owned(),
            external_ids,
        }
    }

    /// Public game metadata only; safe for diagnostics, never credentials.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Whether a source adapter supplied a numeric platform ID to the worker.
    pub fn has_platform_identity(&self) -> bool {
        !self.external_ids.is_empty()
    }
}

/// Consumes an owned lookup plan on a worker thread. Only this part performs
/// HTTP requests; it does not hold a non-Send source registry or UI handle.
pub struct SteamGridDbMatchService {
    client: SteamGridDbClient,
}

impl SteamGridDbMatchService {
    pub fn new(client: SteamGridDbClient) -> Self {
        Self { client }
    }

    pub fn resolve(&self, lookup: &SteamGridDbLookup) -> Result<MatchOutcome, SteamGridDbError> {
        if !lookup.external_ids.is_empty() {
            let mut matches = Vec::new();
            for external in &lookup.external_ids {
                if let Some(found) = self.client.game_by_external_id(external)?
                    && !matches
                        .iter()
                        .any(|prev: &SteamGridDbGame| prev.id == found.id)
                {
                    matches.push(found);
                }
            }
            match matches.len() {
                1 => {
                    return Ok(MatchOutcome::Matched(SteamGridDbMatch {
                        game: matches.remove(0),
                        method: MatchMethod::ExactPlatformId,
                    }));
                }
                // An actual conflict between platform IDs is not safe to guess.
                n if n > 1 => return Ok(MatchOutcome::Ambiguous),
                _ => {}
            }
        }

        // SteamGridDB may not index some valid Steam AppIDs. A unique,
        // normalized *exact* title match is safe to use when every source ID
        // lookup returned NotFound; near matches and ambiguous titles are not.
        let candidates = self.client.search_by_name(&lookup.title)?;
        Ok(match unique_exact_title(&lookup.title, candidates) {
            TitleMatch::Unique(found) => MatchOutcome::Matched(SteamGridDbMatch {
                game: found,
                method: MatchMethod::UniqueExactTitle,
            }),
            TitleMatch::None => MatchOutcome::NotFound,
            TitleMatch::Ambiguous => MatchOutcome::Ambiguous,
        })
    }

    pub fn square_grids(
        &self,
        game: &SteamGridDbMatch,
    ) -> Result<Vec<SteamGridDbSquareGrid>, SteamGridDbError> {
        self.client.square_grids(game.game.id)
    }

    pub fn square_icons(
        &self,
        game: &SteamGridDbMatch,
    ) -> Result<Vec<SteamGridDbSquareGrid>, SteamGridDbError> {
        self.client.square_icons(game.game.id)
    }

    pub fn download_square_grid(
        &self,
        grid: &SteamGridDbSquareGrid,
    ) -> Result<RgbaImage, SteamGridDbError> {
        self.client.download_square_grid(grid)
    }
}

enum TitleMatch {
    None,
    Unique(SteamGridDbGame),
    Ambiguous,
}

fn unique_exact_title(title: &str, candidates: Vec<SteamGridDbGame>) -> TitleMatch {
    let expected = normalized_title(title);
    let mut matches = candidates
        .into_iter()
        .filter(|candidate| normalized_title(&candidate.name) == expected);
    let Some(first) = matches.next() else {
        return TitleMatch::None;
    };
    // Duplicate rows for the SAME SteamGridDB ID are not ambiguous.
    if matches.any(|match_| match_.id != first.id) {
        return TitleMatch::Ambiguous;
    }
    TitleMatch::Unique(first)
}

fn normalized_title(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ExternalGameId, Game, GameId, GameTitle, SourceGameRef, SourceId};
    use crate::sources::{
        GameSource, SourceDescriptor, SourceDiscovery, SourceError, SourceSnapshot,
    };

    struct MockSource(SourceDescriptor);

    impl GameSource for MockSource {
        fn descriptor(&self) -> &SourceDescriptor {
            &self.0
        }
        fn discover(&self) -> Result<SourceDiscovery, SourceError> {
            Ok(SourceDiscovery::Available(
                SourceSnapshot::new(vec![]).unwrap(),
            ))
        }
    }

    fn game(name: &str, source: &str, id: &str) -> LibraryGame {
        LibraryGame::new(
            Game::new(GameId::new(1).unwrap(), GameTitle::new(name).unwrap()),
            vec![SourceGameRef::new(
                SourceId::new(source).unwrap(),
                ExternalGameId::new(id).unwrap(),
            )],
        )
    }

    #[test]
    fn square_artwork_ranks_highest_vote_score_first_even_if_uploaded_later() {
        let items: Vec<ApiGrid> = serde_json::from_str(
            r#"[
            {"id": 11, "url":"https://cdn2.steamgriddb.com/grid/11.png", "score": 2},
            {"id": 10, "url":"https://cdn2.steamgriddb.com/grid/10.png", "score": 41},
            {"id": 12, "url":"https://cdn2.steamgriddb.com/grid/12.png", "score": -1},
            {"id": 15, "url":"https://cdn2.steamgriddb.com/grid/15.png"},
            {"id": 13, "url":"https://cdn2.steamgriddb.com/grid/13.png", "score": 41},
            {"id": 14, "url":"http://other.example/grid.png", "score": 999}
        ]"#,
        )
        .unwrap();
        let result = sorted_eligible_artwork(items);
        assert_eq!(
            result.iter().map(|g| g.id).collect::<Vec<_>>(),
            [10, 13, 11, 12, 15]
        );
        assert_eq!(result[0].score, Some(41));
    }

    #[test]
    fn strict_name_matching_refuses_near_matches_and_collisions() {
        let a = SteamGridDbGame {
            id: 3,
            name: "Game: Deluxe".into(),
            verified: true,
        };
        let b = SteamGridDbGame {
            id: 4,
            name: "GAME DELUXE".into(),
            verified: true,
        };
        assert!(matches!(
            unique_exact_title("Game: Deluxe", vec![a.clone(), b]),
            TitleMatch::Unique(_)
        ));
        assert!(matches!(
            unique_exact_title("Game Deluxe", vec![a.clone()]),
            TitleMatch::None
        ));
        assert!(matches!(
            unique_exact_title(" game:   deluxe ", vec![a.clone()]),
            TitleMatch::Unique(_)
        ));
        assert!(matches!(
            unique_exact_title(
                "Game: Deluxe",
                vec![a.clone(), SteamGridDbGame { id: 9, ..a }]
            ),
            TitleMatch::Ambiguous
        ));
    }

    #[test]
    fn square_icon_256_decodes_but_non_square_images_are_rejected() {
        let square = image::DynamicImage::ImageRgba8(image::RgbaImage::new(256, 256));
        let mut out = Cursor::new(Vec::new());
        square.write_to(&mut out, ImageFormat::Png).unwrap();
        assert_eq!(
            decode_square_grid(&out.into_inner()).unwrap().dimensions(),
            (256, 256)
        );
        let portrait = image::DynamicImage::ImageRgba8(image::RgbaImage::new(256, 384));
        let mut out = Cursor::new(Vec::new());
        portrait.write_to(&mut out, ImageFormat::Png).unwrap();
        assert!(decode_square_grid(&out.into_inner()).is_err());
    }

    #[test]
    fn url_allowlist_rejects_credentials_http_and_lookalike_domains() {
        assert!(is_steamgriddb_https(
            &Url::parse("https://cdn2.steamgriddb.com/grid/example.png").unwrap()
        ));
        assert!(!is_steamgriddb_https(
            &Url::parse("http://cdn2.steamgriddb.com/grid/example.png").unwrap()
        ));
        assert!(!is_steamgriddb_https(
            &Url::parse("https://steamgriddb.com.evil.test/grid/example.png").unwrap()
        ));
        assert!(!is_steamgriddb_https(
            &Url::parse("https://someone@cdn2.steamgriddb.com/grid/example.png").unwrap()
        ));
        assert!(!is_steamgriddb_https(
            &Url::parse("https://cdn2.steamgriddb.com:4443/grid/example.png").unwrap()
        ));
    }

    #[test]
    fn nonsteam_sources_have_no_guessed_external_catalog_identity() {
        let mut registry = SourceRegistry::new();
        registry
            .register(MockSource(
                SourceDescriptor::new(SourceId::new("bottles").unwrap(), "Bottles", vec![])
                    .unwrap(),
            ))
            .unwrap();
        let reference = game("Example Game", "bottles", "native:my-bottle/123");
        let lookup = SteamGridDbLookup::for_game(&reference, &registry);
        assert!(lookup.external_ids.is_empty());
        assert_eq!(lookup.title, "Example Game");
    }

    #[test]
    fn steam_source_publishes_authoritative_id_for_lookup_plan() {
        let mut registry = SourceRegistry::new();
        registry
            .register(crate::sources::steam::SteamSource::new().unwrap())
            .unwrap();
        let reference = game("Demo", "steam", "480");
        let lookup = SteamGridDbLookup::for_game(&reference, &registry);
        assert_eq!(
            lookup.external_ids,
            vec![ExternalArtworkId::SteamAppId(480)]
        );
        // Steam's app ID is always attempted first; only a missing mapping
        // can fall back to a unique exact-title match.
        assert!(!lookup.external_ids.is_empty());
    }

    /// Exercise request path, credential header, status classification and
    /// JSON deserialization without touching the real SteamGridDB service.
    fn serve_once(status: u16, body: &'static str) -> (Url, std::thread::JoinHandle<String>) {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0u8; 4096];
            let n = stream.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..n]).into_owned();
            write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            stream.flush().unwrap();
            request
        });
        (
            Url::parse(&format!("http://{address}/api/v2/")).unwrap(),
            handle,
        )
    }

    fn test_client(url: Url) -> SteamGridDbClient {
        let mut client = SteamGridDbClient::new("DO_NOT_LOG_TEST_KEY").unwrap();
        client.base = url;
        client
    }

    #[test]
    fn api_client_uses_bearer_header_and_parses_exact_game() {
        let (url, handle) = serve_once(
            200,
            r#"{"success":true,"data":{"id":17,"name":"Example","verified":true}}"#,
        );
        let game = test_client(url)
            .game_by_external_id(&ExternalArtworkId::SteamAppId(480))
            .unwrap()
            .unwrap();
        assert_eq!(game.id, 17);
        assert_eq!(game.name, "Example");
        let request = handle.join().unwrap();
        assert!(request.starts_with("GET /api/v2/games/steam/480 HTTP/1.1"));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer do_not_log_test_key")
        );
    }

    #[test]
    fn api_errors_and_missing_game_are_separate() {
        let (url, handle) = serve_once(404, "{}");
        let absent = test_client(url)
            .game_by_external_id(&ExternalArtworkId::SteamAppId(480))
            .unwrap();
        assert!(absent.is_none());
        handle.join().unwrap();
        let (url, handle) = serve_once(401, "{}");
        let error = test_client(url)
            .game_by_external_id(&ExternalArtworkId::SteamAppId(480))
            .unwrap_err();
        assert!(matches!(error, SteamGridDbError::Unauthorized));
        assert!(!error.to_string().contains("DO_NOT_LOG_TEST_KEY"));
        handle.join().unwrap();
        let (url, handle) = serve_once(429, "{}");
        let error = test_client(url)
            .game_by_external_id(&ExternalArtworkId::SteamAppId(480))
            .unwrap_err();
        assert!(matches!(error, SteamGridDbError::RateLimited));
        handle.join().unwrap();
    }

    #[test]
    fn grid_metadata_filters_off_domain_image_urls() {
        let (url, handle) = serve_once(
            200,
            r#"{"success":true,"data":[{"id":1,"url":"https://cdn2.steamgriddb.com/grid/ok.png","author":{"name":"Contributor"}},{"id":2,"url":"https://elsewhere.invalid/bad.png"}]}"#,
        );
        let grids = test_client(url).square_grids(17).unwrap();
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].id, 1);
        assert_eq!(grids[0].author.as_deref(), Some("Contributor"));
        let request = handle.join().unwrap();
        assert!(request.starts_with("GET /api/v2/grids/game/17?"));
        assert!(request.contains("dimensions=512x512%2C1024x1024"));
    }

    #[test]
    fn search_handles_both_documented_response_shapes() {
        let direct =
            r#"[{"id":17,"name":"Example"},{"success":true,"data":{"id":18,"name":"Example 2"}}]"#;
        let hits: Vec<ApiSearchHit> = serde_json::from_str(direct).unwrap();
        let games = hits
            .into_iter()
            .map(|h| match h {
                ApiSearchHit::Direct(game) | ApiSearchHit::Wrapped { data: game } => game,
            })
            .collect::<Vec<_>>();
        assert_eq!(games.iter().map(|g| g.id).collect::<Vec<_>>(), vec![17, 18]);
    }

    #[test]
    fn downloaded_art_must_actually_be_square_png_or_jpeg() {
        let good = image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            512,
            512,
            image::Rgba([10, 20, 30, 255]),
        ));
        let mut encoded = Cursor::new(Vec::new());
        good.write_to(&mut encoded, ImageFormat::Png).unwrap();
        assert_eq!(
            decode_square_grid(encoded.get_ref()).unwrap().dimensions(),
            (512, 512)
        );
        let bad = image::DynamicImage::ImageRgba8(RgbaImage::new(600, 900));
        let mut encoded = Cursor::new(Vec::new());
        bad.write_to(&mut encoded, ImageFormat::Png).unwrap();
        assert!(matches!(
            decode_square_grid(encoded.get_ref()),
            Err(SteamGridDbError::InvalidImage)
        ));
    }

    #[test]
    fn api_paths_have_one_separator_after_v2_for_every_endpoint() {
        let client = SteamGridDbClient::new("dummy").unwrap();
        assert_eq!(
            client.api_url(&["games", "steam", "1055540"]).as_str(),
            "https://www.steamgriddb.com/api/v2/games/steam/1055540"
        );
        assert_eq!(
            client.api_url(&["grids", "game", "2978157"]).as_str(),
            "https://www.steamgriddb.com/api/v2/grids/game/2978157"
        );
        assert_eq!(
            client.api_url(&["icons", "game", "2978157"]).as_str(),
            "https://www.steamgriddb.com/api/v2/icons/game/2978157"
        );
        assert_eq!(
            client
                .api_url(&["search", "autocomplete", "A Short Hike"])
                .as_str(),
            "https://www.steamgriddb.com/api/v2/search/autocomplete/A%20Short%20Hike"
        );
    }

    #[test]
    fn names_are_encoded_as_single_url_path_segments() {
        let client = SteamGridDbClient::new("dummy").unwrap();
        let url = client.api_url(&["search", "autocomplete", "Foo / Bar ? #"]);
        assert!(url.path().ends_with("/Foo%20%2F%20Bar%20%3F%20%23"));
        assert!(url.query().is_none());
    }
}
