use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum EntityType {
    Person,
    Project,
    Term,
    Uncertain,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DetectedEntity {
    pub name: String,
    pub r#type: EntityType,
    pub confidence: f32,
    pub signals: Vec<String>,
    pub aliases: Vec<String>,
    pub relationship: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum MemoryType {
    Decision,
    Preference,
    Milestone,
    Problem,
    Emotional,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Wing {
    pub name: String,
    pub r#type: String,
    pub keywords: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Room {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Drawer {
    pub id: String,
    pub content: String,
    pub metadata: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_wing_creation() {
        let wing = Wing {
            name: "test".to_string(),
            r#type: "project".to_string(),
            keywords: vec!["rust".to_string()],
        };
        assert_eq!(wing.name, "test");
        assert_eq!(wing.keywords[0], "rust");
    }

    #[test]
    fn test_drawer_metadata() {
        let drawer = Drawer {
            id: "1".to_string(),
            content: "content".to_string(),
            metadata: json!({"wing": "general"}),
        };
        assert_eq!(drawer.metadata["wing"], "general");
    }

    #[test]
    fn test_room_creation() {
        let room = Room {
            name: "test".to_string(),
            description: Some("desc".to_string()),
        };
        assert_eq!(room.name, "test");
        assert_eq!(room.description.unwrap(), "desc");
    }

    #[test]
    fn test_model_serialization() {
        let wing = Wing {
            name: "w".into(),
            r#type: "t".into(),
            keywords: vec![],
        };
        let wing_json = serde_json::to_string(&wing).unwrap();
        let wing_de: Wing = serde_json::from_str(&wing_json).unwrap();
        assert_eq!(wing_de.name, "w");

        let long_wing = Wing {
            name: "A".repeat(1000),
            r#type: "t".into(),
            keywords: vec![],
        };
        assert_eq!(long_wing.name.len(), 1000);

        let room = Room {
            name: "r".into(),
            description: None,
        };
        let room_json = serde_json::to_string(&room).unwrap();
        let room_de: Room = serde_json::from_str(&room_json).unwrap();
        assert_eq!(room_de.name, "r");

        let drawer = Drawer {
            id: "d".into(),
            content: "c".into(),
            metadata: json!({}),
        };
        let drawer_json = serde_json::to_string(&drawer).unwrap();
        let drawer_de: Drawer = serde_json::from_str(&drawer_json).unwrap();
        assert_eq!(drawer_de.id, "d");
    }
}
