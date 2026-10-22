//! Presentation/search names; catalog names and ROM identities stay unchanged.

/// Remove catalog annotations from display names, retaining meaningful title text.
pub fn display_title(title: &str) -> String {
    let mut result = String::new();
    let mut remaining = title;
    while let Some(start) = remaining.find(" (") {
        result.push_str(&remaining[..start]);
        let group = &remaining[start + 2..];
        let Some(end) = group.find(')') else {
            result.push_str(&remaining[start..]);
            return result.trim().to_owned();
        };
        if !is_release_metadata(&group[..end]) || result.trim().is_empty() {
            result.push_str(&remaining[start..start + end + 3]);
        }
        remaining = &group[end + 1..];
    }
    result.push_str(remaining);
    result.trim().to_owned()
}

/// Artwork may omit known trailing annotations, but never guess away an edition,
/// hack name or unknown tag. Retain those to prevent matching a different game.
pub fn artwork_title(title: &str) -> Option<String> {
    let mut search = title.trim();
    while let Some(without_close) = search.strip_suffix(')') {
        let Some((base, tag)) = without_close.rsplit_once(" (") else {
            break;
        };
        if base.trim().is_empty() || !is_release_metadata(tag) {
            break;
        }
        search = base.trim_end();
    }
    (search != title.trim()).then(|| search.to_owned())
}

fn is_release_metadata(tag: &str) -> bool {
    let tag = tag.to_ascii_lowercase();
    if let Some(revision) = tag.strip_prefix("rev ") {
        return !revision.is_empty() && revision.chars().all(|c| c.is_ascii_digit());
    }
    if let Some(version) = tag.strip_prefix('v') {
        let mut parts = version.split('.');
        if parts.clone().count() >= 2
            && parts.all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        {
            return true;
        }
    }
    tag.split(',').all(|part| {
        matches!(
            part.trim(),
            "usa"
                | "europe"
                | "japan"
                | "world"
                | "australia"
                | "canada"
                | "france"
                | "germany"
                | "italy"
                | "spain"
                | "netherlands"
                | "sweden"
                | "korea"
                | "china"
                | "brazil"
                | "taiwan"
                | "russia"
                | "united kingdom"
                | "asia"
                | "hong kong"
                | "denmark"
                | "norway"
                | "finland"
                | "portugal"
                | "poland"
                | "greece"
                | "israel"
                | "en"
                | "fr"
                | "de"
                | "es"
                | "it"
                | "ja"
                | "nl"
                | "pt"
                | "sv"
                | "da"
                | "fi"
                | "no"
                | "ko"
                | "zh"
                | "ru"
                | "pl"
                | "cs"
                | "hu"
                | "tr"
                | "el"
                | "he"
                | "ar"
                | "pt-br"
                | "gb"
                | "gbc"
                | "gba"
                | "nds"
                | "ndsi"
                | "gb compatible"
                | "sgb enhanced"
                | "cgb enhanced"
                | "cgb+sgb enhanced"
                | "ndsi enhanced"
                | "virtual console"
                | "wii u virtual console"
                | "nintendo switch online"
                | "3ds virtual console"
                | "rumble version"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_language_region_and_revision_annotations_leave_only_the_title() {
        for (raw, expected) in [
            (
                "Pokemon - Gold Version (USA, Europe) (SGB Enhanced) (GB Compatible)",
                "Pokemon - Gold Version",
            ),
            (
                "Pokemon - Black Version 2 (USA, Europe) (NDSi Enhanced)",
                "Pokemon - Black Version 2",
            ),
            (
                "Game (Europe) (En,Fr,De,Es,It,Nl,Pt,Sv,No,Da,Fi) (Rev 1) (NDS)",
                "Game",
            ),
            ("Game (Taiwan) (Zh) (CGB+SGB Enhanced)", "Game"),
            ("Game (USA) (Wii U Virtual Console) (v1.2.3)", "Game"),
            ("Game (SGB Enhanced, GB Compatible)", "Game"),
            ("Game (Special Edition) (Japan)", "Game (Special Edition)"),
        ] {
            assert_eq!(display_title(raw), expected);
            assert_eq!(artwork_title(raw).as_deref(), Some(expected));
        }
    }

    #[test]
    fn preserves_real_titles_editions_hacks_and_unknown_annotations() {
        for raw in [
            "Game",
            "Game (Beta)",
            "Game (Hack)",
            "Game (Special Edition)",
            "Game (Unknown)",
            "Game (USA",
            "Game (v1.)",
        ] {
            assert_eq!(display_title(raw), raw);
            assert_eq!(artwork_title(raw), None);
        }
        assert_eq!(display_title(" (USA)"), "(USA)");
        assert_eq!(artwork_title(" (USA)"), None);
        assert_eq!(display_title("Game (USA) Deluxe"), "Game Deluxe");
        assert_eq!(artwork_title("Game (USA) Deluxe"), None);
        assert_eq!(
            artwork_title("Pokemon - Recharged Yellow (USA)").as_deref(),
            Some("Pokemon - Recharged Yellow")
        );
    }
}
