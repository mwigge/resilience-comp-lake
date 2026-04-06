use serde::{Deserialize, Serialize};

use super::framework::FrameworkId;

/// Unique identifier for a control within a framework.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ControlId(String);

impl ControlId {
    /// Create a new `ControlId`, returning an error if the value is empty.
    ///
    /// # Errors
    ///
    /// Returns `ControlIdError::Empty` if the provided string is empty.
    #[must_use = "returns the validated ControlId"]
    pub fn new(id: impl Into<String>) -> Result<Self, ControlIdError> {
        let id = id.into();
        if id.is_empty() {
            return Err(ControlIdError::Empty);
        }
        Ok(Self(id))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ControlId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ControlIdError {
    #[error("control ID must not be empty")]
    Empty,
}

/// Severity level of a control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    High,
    Moderate,
    Low,
}

/// Logical grouping of controls within a framework.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ControlFamily(String);

impl ControlFamily {
    #[must_use]
    pub fn new(family: impl Into<String>) -> Self {
        Self(family.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ControlFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A single control (article, requirement, safeguard) within a framework.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Control {
    pub control_id: ControlId,
    pub framework_id: FrameworkId,
    pub article_ref: Option<String>,
    pub chapter_ref: Option<String>,
    pub title: String,
    pub description: String,
    pub family: Option<ControlFamily>,
    pub severity: Severity,
    pub testing_relevant: bool,
    pub parent_id: Option<ControlId>,
}

impl Control {
    #[must_use]
    pub fn builder(
        control_id: ControlId,
        framework_id: FrameworkId,
        title: impl Into<String>,
    ) -> ControlBuilder {
        ControlBuilder {
            control_id,
            framework_id,
            article_ref: None,
            chapter_ref: None,
            title: title.into(),
            description: String::new(),
            family: None,
            severity: Severity::Moderate,
            testing_relevant: true,
            parent_id: None,
        }
    }
}

pub struct ControlBuilder {
    control_id: ControlId,
    framework_id: FrameworkId,
    article_ref: Option<String>,
    chapter_ref: Option<String>,
    title: String,
    description: String,
    family: Option<ControlFamily>,
    severity: Severity,
    testing_relevant: bool,
    parent_id: Option<ControlId>,
}

impl ControlBuilder {
    #[must_use]
    pub fn article_ref(mut self, r: impl Into<String>) -> Self {
        self.article_ref = Some(r.into());
        self
    }

    #[must_use]
    pub fn chapter_ref(mut self, r: impl Into<String>) -> Self {
        self.chapter_ref = Some(r.into());
        self
    }

    #[must_use]
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = d.into();
        self
    }

    #[must_use]
    pub fn family(mut self, f: ControlFamily) -> Self {
        self.family = Some(f);
        self
    }

    #[must_use]
    pub fn severity(mut self, s: Severity) -> Self {
        self.severity = s;
        self
    }

    #[must_use]
    pub fn testing_relevant(mut self, t: bool) -> Self {
        self.testing_relevant = t;
        self
    }

    #[must_use]
    pub fn parent_id(mut self, p: ControlId) -> Self {
        self.parent_id = Some(p);
        self
    }

    #[must_use]
    pub fn build(self) -> Control {
        Control {
            control_id: self.control_id,
            framework_id: self.framework_id,
            article_ref: self.article_ref,
            chapter_ref: self.chapter_ref,
            title: self.title,
            description: self.description,
            family: self.family,
            severity: self.severity,
            testing_relevant: self.testing_relevant,
            parent_id: self.parent_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_id_rejects_empty() {
        assert!(ControlId::new("").is_err());
    }

    #[test]
    fn control_id_accepts_valid() {
        let id = ControlId::new("DORA-ART-25").unwrap();
        assert_eq!(id.as_str(), "DORA-ART-25");
    }

    #[test]
    fn control_serde_roundtrip() {
        let ctrl = Control::builder(
            ControlId::new("DORA-ART-25").unwrap(),
            FrameworkId::new("DORA").unwrap(),
            "ICT-related incident management",
        )
        .article_ref("Art. 25")
        .severity(Severity::High)
        .family(ControlFamily::new("Incident Management"))
        .description("Requirements for ICT incident detection and response")
        .build();

        let json = serde_json::to_string(&ctrl).unwrap();
        let deserialized: Control = serde_json::from_str(&json).unwrap();
        assert_eq!(ctrl, deserialized);
    }

    #[test]
    fn severity_serde_roundtrip() {
        for sev in [Severity::High, Severity::Moderate, Severity::Low] {
            let json = serde_json::to_string(&sev).unwrap();
            let deserialized: Severity = serde_json::from_str(&json).unwrap();
            assert_eq!(sev, deserialized);
        }
    }
}
