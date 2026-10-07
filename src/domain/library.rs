use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainValidationError {
    #[error("game id must be positive, got {0}")]
    InvalidGameId(i64),
    #[error("game title must not be empty")]
    EmptyGameTitle,
    #[error("source id must not be empty")]
    EmptySourceId,
    #[error("external game id must not be empty")]
    EmptyExternalGameId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GameId(i64);

impl GameId {
    pub fn new(value: i64) -> Result<Self, DomainValidationError> {
        if value <= 0 {
            return Err(DomainValidationError::InvalidGameId(value));
        }

        Ok(Self(value))
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GameTitle(String);

impl GameTitle {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainValidationError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(DomainValidationError::EmptyGameTitle);
        }

        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(String);

impl SourceId {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainValidationError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DomainValidationError::EmptySourceId);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExternalGameId(String);

impl ExternalGameId {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainValidationError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DomainValidationError::EmptyExternalGameId);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    id: GameId,
    title: GameTitle,
}

impl Game {
    pub fn new(id: GameId, title: GameTitle) -> Self {
        Self { id, title }
    }

    pub const fn id(&self) -> GameId {
        self.id
    }

    pub fn title(&self) -> &GameTitle {
        &self.title
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceGameRef {
    source_id: SourceId,
    external_id: ExternalGameId,
}

impl SourceGameRef {
    pub fn new(source_id: SourceId, external_id: ExternalGameId) -> Self {
        Self {
            source_id,
            external_id,
        }
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub fn external_id(&self) -> &ExternalGameId {
        &self.external_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryGame {
    game: Game,
    sources: Vec<SourceGameRef>,
}

impl LibraryGame {
    pub fn new(game: Game, sources: Vec<SourceGameRef>) -> Self {
        Self { game, sources }
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn sources(&self) -> &[SourceGameRef] {
        &self.sources
    }

    pub(crate) fn push_source(&mut self, source: SourceGameRef) {
        self.sources.push(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_trimmed_and_must_not_be_empty() {
        let title = GameTitle::new("  Solar Drift  ").expect("valid title");
        assert_eq!(title.as_str(), "Solar Drift");
        assert_eq!(
            GameTitle::new("  ").expect_err("blank title must fail"),
            DomainValidationError::EmptyGameTitle
        );
    }

    #[test]
    fn source_identity_must_not_be_blank_but_preserves_value() {
        let source = SourceId::new("steam").expect("valid source id");
        let external = ExternalGameId::new("480").expect("valid external id");
        assert_eq!(source.as_str(), "steam");
        assert_eq!(external.as_str(), "480");

        assert_eq!(
            SourceId::new("\t").expect_err("blank source id must fail"),
            DomainValidationError::EmptySourceId
        );
        assert_eq!(
            ExternalGameId::new("\n").expect_err("blank external id must fail"),
            DomainValidationError::EmptyExternalGameId
        );
    }

    #[test]
    fn game_ids_are_database_safe_positive_values() {
        assert_eq!(GameId::new(7).expect("valid id").get(), 7);
        assert_eq!(
            GameId::new(0).expect_err("zero id must fail"),
            DomainValidationError::InvalidGameId(0)
        );
    }
}
