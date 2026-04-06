use serde::{Deserialize, Serialize};

/// Unique identifier for an organisational entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(String);

impl EntityId {
    /// Create a new `EntityId`.
    ///
    /// # Panics
    ///
    /// Panics if `id` is empty. Use `try_new` for fallible construction.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        assert!(!id.is_empty(), "EntityId must not be empty");
        Self(id)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Level in the organisational hierarchy.
///
/// Ordering: Platform > Unit > Team > Project (higher = broader scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityType {
    Platform,
    Unit,
    Team,
    Project,
}

impl EntityType {
    #[must_use]
    fn rank(self) -> u8 {
        match self {
            Self::Platform => 3,
            Self::Unit => 2,
            Self::Team => 1,
            Self::Project => 0,
        }
    }
}

impl Ord for EntityType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for EntityType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// An organisational entity (platform, business unit, team, or project).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrgEntity {
    pub entity_id: EntityId,
    pub entity_type: EntityType,
    pub name: String,
    pub parent_id: Option<EntityId>,
}

impl OrgEntity {
    #[must_use]
    pub fn new(
        entity_id: EntityId,
        entity_type: EntityType,
        name: impl Into<String>,
        parent_id: Option<EntityId>,
    ) -> Self {
        Self {
            entity_id,
            entity_type,
            name: name.into(),
            parent_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_type_ordering() {
        assert!(EntityType::Platform > EntityType::Unit);
        assert!(EntityType::Unit > EntityType::Team);
        assert!(EntityType::Team > EntityType::Project);
        assert!(EntityType::Platform > EntityType::Project);
    }

    #[test]
    fn entity_type_serde_roundtrip() {
        for et in [
            EntityType::Platform,
            EntityType::Unit,
            EntityType::Team,
            EntityType::Project,
        ] {
            let json = serde_json::to_string(&et).unwrap();
            let deserialized: EntityType = serde_json::from_str(&json).unwrap();
            assert_eq!(et, deserialized);
        }
    }

    #[test]
    fn org_entity_serde_roundtrip() {
        let entity = OrgEntity::new(
            EntityId::new("team-alpha"),
            EntityType::Team,
            "Team Alpha",
            Some(EntityId::new("unit-engineering")),
        );

        let json = serde_json::to_string(&entity).unwrap();
        let deserialized: OrgEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(entity, deserialized);
    }
}
