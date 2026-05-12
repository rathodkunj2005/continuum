use super::extract_domain;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CaptureSurfacePolicy {
    Normal,
    UrlOnly,
    SkipFrame,
}

pub(super) fn classify_capture_surface_policy(
    app_name: &str,
    window_title: &str,
    url: Option<&str>,
) -> CaptureSurfacePolicy {
    if !is_browser_app(app_name) {
        return CaptureSurfacePolicy::Normal;
    }
    let Some(url) = url else {
        return CaptureSurfacePolicy::Normal;
    };

    let title = window_title.to_ascii_lowercase();
    if is_generic_browser_chrome_title(&title) {
        return CaptureSurfacePolicy::SkipFrame;
    }

    let surface = UrlSurface::from_url(url);
    if is_navigation_surface(&surface) {
        return CaptureSurfacePolicy::SkipFrame;
    }
    if is_listing_surface(&surface, &title) {
        return CaptureSurfacePolicy::UrlOnly;
    }
    CaptureSurfacePolicy::Normal
}

fn is_browser_app(app_name: &str) -> bool {
    let app = app_name.to_ascii_lowercase();
    [
        "chrome", "safari", "firefox", "arc", "edge", "brave", "opera",
    ]
    .iter()
    .any(|needle| app.contains(needle))
}

fn is_generic_browser_chrome_title(title: &str) -> bool {
    ["new tab", "start page", "speed dial", "blank page"]
        .iter()
        .any(|needle| title.contains(needle))
}

#[derive(Debug, Clone)]
struct UrlSurface {
    domain: String,
    path: String,
    path_segments: Vec<String>,
    query_keys: Vec<String>,
}

impl UrlSurface {
    fn from_url(url: &str) -> Self {
        let lower_url = url.to_ascii_lowercase();
        let without_scheme = lower_url
            .split("://")
            .nth(1)
            .unwrap_or(lower_url.as_str())
            .split('#')
            .next()
            .unwrap_or_default();

        let path_and_query = without_scheme
            .split_once('/')
            .map(|(_, rest)| format!("/{}", rest))
            .unwrap_or_else(|| "/".to_string());
        let (path_raw, query_raw) = path_and_query
            .split_once('?')
            .map(|(path, query)| (path, query))
            .unwrap_or((path_and_query.as_str(), ""));

        let path = if path_raw.is_empty() {
            "/".to_string()
        } else {
            path_raw.to_string()
        };
        let path_segments = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let query_keys = query_raw
            .split('&')
            .filter_map(|entry| entry.split_once('=').map(|(key, _)| key.to_string()))
            .collect::<Vec<_>>();

        Self {
            domain: extract_domain(url).unwrap_or_default().to_ascii_lowercase(),
            path,
            path_segments,
            query_keys,
        }
    }
}

fn contains_path_segment(surface: &UrlSurface, candidates: &[&str]) -> bool {
    surface
        .path_segments
        .iter()
        .any(|segment| candidates.iter().any(|candidate| segment == candidate))
}

fn contains_search_query_key(surface: &UrlSurface) -> bool {
    surface.query_keys.iter().any(|key| {
        matches!(
            key.as_str(),
            "q" | "query" | "search" | "search_query" | "text" | "term"
        )
    })
}

fn is_navigation_surface(surface: &UrlSurface) -> bool {
    if contains_search_query_key(surface)
        && (contains_path_segment(surface, &["search", "results"])
            || surface.path.contains("/search")
            || surface.path.contains("/results"))
    {
        return true;
    }

    contains_path_segment(
        surface,
        &["feed", "explore", "discover", "home", "trending", "hashtag"],
    )
}

fn is_listing_surface(surface: &UrlSurface, title: &str) -> bool {
    if title.contains("search results") || title.contains("videos -") {
        return true;
    }

    let primary_segment = surface
        .path_segments
        .first()
        .map(String::as_str)
        .unwrap_or("");
    if primary_segment.starts_with('@')
        || matches!(
            primary_segment,
            "u" | "user"
                | "users"
                | "profile"
                | "profiles"
                | "channel"
                | "channels"
                | "topic"
                | "topics"
                | "tag"
                | "tags"
        )
    {
        return true;
    }

    if surface.domain.ends_with("youtube.com") && primary_segment == "c" {
        return true;
    }

    let looks_like_collection = contains_path_segment(
        surface,
        &[
            "videos",
            "posts",
            "reels",
            "playlist",
            "playlists",
            "top",
            "best",
            "latest",
        ],
    );
    looks_like_collection && surface.path_segments.len() <= 3
}
