use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique identifier for a compliance framework (e.g. "DORA", "NIST-800-53-R5").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameworkId(String);

impl FrameworkId {
    /// Create a new `FrameworkId`, returning an error if the value is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if the ID is empty or contains invalid characters.
    #[must_use = "returns the validated FrameworkId"]
    pub fn new(id: impl Into<String>) -> Result<Self, FrameworkIdError> {
        let id = id.into();
        if id.is_empty() {
            return Err(FrameworkIdError::Empty);
        }
        if !id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(FrameworkIdError::InvalidChars);
        }
        Ok(Self(id))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FrameworkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameworkIdError {
    #[error("framework ID must not be empty")]
    Empty,
    #[error("framework ID contains invalid characters (only alphanumeric, hyphens, underscores, dots allowed)")]
    InvalidChars,
}

/// Geographic region a framework applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    Eu,
    Global,
    Us,
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eu => f.write_str("Eu"),
            Self::Global => f.write_str("Global"),
            Self::Us => f.write_str("Us"),
        }
    }
}

/// Source from which a framework is harvested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HarvestSource {
    CellarSparql,
    OscalGithub,
    NvdApi,
    ScorecardApi,
    Manual,
}

impl std::fmt::Display for HarvestSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CellarSparql => f.write_str("CellarSparql"),
            Self::OscalGithub => f.write_str("OscalGithub"),
            Self::NvdApi => f.write_str("NvdApi"),
            Self::ScorecardApi => f.write_str("ScorecardApi"),
            Self::Manual => f.write_str("Manual"),
        }
    }
}

/// A compliance framework (e.g. DORA, ISO 27001, NIST 800-53).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Framework {
    pub framework_id: FrameworkId,
    pub name: String,
    pub version: String,
    pub region: Region,
    pub authority: String,
    pub is_pivot: bool,
    pub effective_date: Option<DateTime<Utc>>,
    pub sunset_date: Option<DateTime<Utc>>,
    pub celex_id: Option<String>,
    pub eli_uri: Option<String>,
    pub harvest_source: HarvestSource,
    pub last_harvested: Option<DateTime<Utc>>,
}

impl Framework {
    #[must_use]
    pub fn builder(id: FrameworkId, name: impl Into<String>) -> FrameworkBuilder {
        FrameworkBuilder {
            framework_id: id,
            name: name.into(),
            version: String::new(),
            region: Region::Global,
            authority: String::new(),
            is_pivot: false,
            effective_date: None,
            sunset_date: None,
            celex_id: None,
            eli_uri: None,
            harvest_source: HarvestSource::Manual,
            last_harvested: None,
        }
    }
}

pub struct FrameworkBuilder {
    framework_id: FrameworkId,
    name: String,
    version: String,
    region: Region,
    authority: String,
    is_pivot: bool,
    effective_date: Option<DateTime<Utc>>,
    sunset_date: Option<DateTime<Utc>>,
    celex_id: Option<String>,
    eli_uri: Option<String>,
    harvest_source: HarvestSource,
    last_harvested: Option<DateTime<Utc>>,
}

impl FrameworkBuilder {
    #[must_use]
    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = v.into();
        self
    }

    #[must_use]
    pub fn region(mut self, r: Region) -> Self {
        self.region = r;
        self
    }

    #[must_use]
    pub fn authority(mut self, a: impl Into<String>) -> Self {
        self.authority = a.into();
        self
    }

    #[must_use]
    pub fn is_pivot(mut self, p: bool) -> Self {
        self.is_pivot = p;
        self
    }

    #[must_use]
    pub fn effective_date(mut self, d: DateTime<Utc>) -> Self {
        self.effective_date = Some(d);
        self
    }

    #[must_use]
    pub fn sunset_date(mut self, d: DateTime<Utc>) -> Self {
        self.sunset_date = Some(d);
        self
    }

    #[must_use]
    pub fn celex_id(mut self, c: impl Into<String>) -> Self {
        self.celex_id = Some(c.into());
        self
    }

    #[must_use]
    pub fn eli_uri(mut self, e: impl Into<String>) -> Self {
        self.eli_uri = Some(e.into());
        self
    }

    #[must_use]
    pub fn harvest_source(mut self, s: HarvestSource) -> Self {
        self.harvest_source = s;
        self
    }

    #[must_use]
    pub fn last_harvested(mut self, d: DateTime<Utc>) -> Self {
        self.last_harvested = Some(d);
        self
    }

    #[must_use]
    pub fn build(self) -> Framework {
        Framework {
            framework_id: self.framework_id,
            name: self.name,
            version: self.version,
            region: self.region,
            authority: self.authority,
            is_pivot: self.is_pivot,
            effective_date: self.effective_date,
            sunset_date: self.sunset_date,
            celex_id: self.celex_id,
            eli_uri: self.eli_uri,
            harvest_source: self.harvest_source,
            last_harvested: self.last_harvested,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framework_id_rejects_empty() {
        assert!(FrameworkId::new("").is_err());
    }

    #[test]
    fn framework_id_accepts_valid() {
        let id = FrameworkId::new("DORA").unwrap();
        assert_eq!(id.as_str(), "DORA");
    }

    #[test]
    fn framework_serde_roundtrip() {
        let fw = Framework::builder(
            FrameworkId::new("DORA").unwrap(),
            "Digital Operational Resilience Act",
        )
        .version("2022/2554")
        .region(Region::Eu)
        .authority("European Parliament")
        .is_pivot(true)
        .harvest_source(HarvestSource::CellarSparql)
        .build();

        let json = serde_json::to_string(&fw).unwrap();
        let deserialized: Framework = serde_json::from_str(&json).unwrap();
        assert_eq!(fw, deserialized);
    }

    #[test]
    fn region_serde_roundtrip() {
        for region in [Region::Eu, Region::Global, Region::Us] {
            let json = serde_json::to_string(&region).unwrap();
            let deserialized: Region = serde_json::from_str(&json).unwrap();
            assert_eq!(region, deserialized);
        }
    }

    #[test]
    fn harvest_source_serde_roundtrip() {
        for src in [
            HarvestSource::CellarSparql,
            HarvestSource::OscalGithub,
            HarvestSource::NvdApi,
            HarvestSource::ScorecardApi,
            HarvestSource::Manual,
        ] {
            let json = serde_json::to_string(&src).unwrap();
            let deserialized: HarvestSource = serde_json::from_str(&json).unwrap();
            assert_eq!(src, deserialized);
        }
    }
}
