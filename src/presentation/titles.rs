//! Display formatting only: keep original catalog names and ROM identities intact.
pub(crate) fn game_title(title: &str) -> String {
    let mut result = String::new();
    let mut remaining = title;
    while let Some(start) = remaining.find(" (") {
        result.push_str(&remaining[..start]);
        let group = &remaining[start + 2..];
        let Some(end) = group.find(')') else {
            result.push_str(&remaining[start..]);
            return result;
        };
        let region = group[..end].split(',').all(|part| {
            matches!(
                part.trim().to_ascii_lowercase().as_str(),
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
            )
        });
        // Never turn a title consisting solely of a region into an empty name.
        if !region || result.trim().is_empty() {
            result.push_str(&remaining[start..start + end + 3]);
        }
        remaining = &group[end + 1..];
    }
    result.push_str(remaining);
    result.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::game_title;

    #[test]
    fn hides_regions_without_hiding_editions_languages_or_revisions() {
        for (original, expected) in [
            (
                "Pokemon - LeafGreen Version (USA)",
                "Pokemon - LeafGreen Version",
            ),
            (
                "Pokemon - Emerald Version (USA, Europe)",
                "Pokemon - Emerald Version",
            ),
            ("Game (Europe) (En,Fr) (Rev 1)", "Game (En,Fr) (Rev 1)"),
            ("Game (Special Edition) (Japan)", "Game (Special Edition)"),
            ("Game (Beta)", "Game (Beta)"),
            ("Game (Unknown)", "Game (Unknown)"),
            ("Game (USA", "Game (USA"),
            (" (USA)", "(USA)"),
        ] {
            assert_eq!(game_title(original), expected);
        }
    }
}
