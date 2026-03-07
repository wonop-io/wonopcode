//! Core data types for the Hierarchical Memory System.

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use super::HmsError;

/// Visibility determines which directories can see a memory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    /// Visible everywhere in the repository.
    #[default]
    Public,
    /// Visible to parent directories only.
    Upstream,
    /// Visible to child directories only.
    Downstream,
    /// Visible only in the defining directory.
    Private,
}

// Custom serializer to output lowercase values
impl Serialize for Visibility {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Visibility::Public => "public",
            Visibility::Upstream => "upstream",
            Visibility::Downstream => "downstream",
            Visibility::Private => "private",
        })
    }
}

// Custom deserializer to support aliases like "children" -> "downstream"
impl<'de> Deserialize<'de> for Visibility {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "public" | "global" => Ok(Visibility::Public),
            "upstream" | "parents" => Ok(Visibility::Upstream),
            "downstream" | "children" => Ok(Visibility::Downstream),
            "private" | "self" => Ok(Visibility::Private),
            other => Err(serde::de::Error::custom(format!(
                "Invalid visibility '{}'. Valid values: public, upstream, downstream, private (aliases: global, parents, children, self)",
                other
            ))),
        }
    }
}

impl std::fmt::Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Visibility::Public => write!(f, "public"),
            Visibility::Upstream => write!(f, "upstream"),
            Visibility::Downstream => write!(f, "downstream"),
            Visibility::Private => write!(f, "private"),
        }
    }
}

impl FromStr for Visibility {
    type Err = HmsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "public" => Ok(Visibility::Public),
            "upstream" => Ok(Visibility::Upstream),
            "downstream" => Ok(Visibility::Downstream),
            "private" => Ok(Visibility::Private),
            _ => Err(HmsError::InvalidVisibility(s.to_string())),
        }
    }
}

/// Storage class determines where memory.yaml files are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StorageClass {
    /// Tracked in git: .wonopcode/memory.yaml
    #[default]
    Tracked,
    /// Not tracked in git: .wonopcode-local/memory.yaml
    Local,
    /// User-global: ~/.config/wonopcode/memory.yaml
    User,
}

impl std::fmt::Display for StorageClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageClass::Tracked => write!(f, "tracked"),
            StorageClass::Local => write!(f, "local"),
            StorageClass::User => write!(f, "user"),
        }
    }
}

impl FromStr for StorageClass {
    type Err = HmsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tracked" => Ok(StorageClass::Tracked),
            "local" => Ok(StorageClass::Local),
            "user" => Ok(StorageClass::User),
            _ => Err(HmsError::InvalidStorageClass(s.to_string())),
        }
    }
}

impl StorageClass {
    /// Get the directory name for this storage class.
    pub fn directory_name(&self) -> &'static str {
        match self {
            StorageClass::Tracked => ".wonopcode",
            StorageClass::Local => ".wonopcode-local",
            StorageClass::User => ".config/wonopcode",
        }
    }

    /// Get the full path to memory.yaml for this storage class.
    ///
    /// For `User` storage, the `base_dir` is ignored and the user's config directory is used.
    pub fn memory_path(&self, base_dir: &Path) -> PathBuf {
        match self {
            StorageClass::Tracked => base_dir.join(".wonopcode/memory.yaml"),
            StorageClass::Local => base_dir.join(".wonopcode-local/memory.yaml"),
            StorageClass::User => dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("~/.config"))
                .join("wonopcode/memory.yaml"),
        }
    }

    /// Get all storage classes in priority order (highest priority first).
    pub fn all_in_priority_order() -> &'static [StorageClass] {
        &[StorageClass::Local, StorageClass::Tracked, StorageClass::User]
    }
}

/// Memory value can be a simple string or structured data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MemoryValue {
    /// A simple string value.
    String(String),
    /// A list of strings.
    List(Vec<String>),
    /// A structured mapping.
    Map(serde_yaml::Mapping),
}

impl MemoryValue {
    /// Convert to string representation for templates.
    pub fn to_string_lossy(&self) -> String {
        match self {
            MemoryValue::String(s) => s.clone(),
            MemoryValue::List(items) => items.join("\n"),
            MemoryValue::Map(map) => serde_yaml::to_string(map).unwrap_or_default(),
        }
    }

    /// Check if this is an empty value.
    pub fn is_empty(&self) -> bool {
        match self {
            MemoryValue::String(s) => s.is_empty(),
            MemoryValue::List(items) => items.is_empty(),
            MemoryValue::Map(map) => map.is_empty(),
        }
    }
}

impl Default for MemoryValue {
    fn default() -> Self {
        MemoryValue::String(String::new())
    }
}

impl From<String> for MemoryValue {
    fn from(s: String) -> Self {
        MemoryValue::String(s)
    }
}

impl From<&str> for MemoryValue {
    fn from(s: &str) -> Self {
        MemoryValue::String(s.to_string())
    }
}

impl From<Vec<String>> for MemoryValue {
    fn from(v: Vec<String>) -> Self {
        MemoryValue::List(v)
    }
}

/// The data portion of a memory entry (without the key).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryEntryData {
    /// The memory value.
    pub value: MemoryValue,
    /// Visibility scope for propagation.
    #[serde(default)]
    pub visibility: Visibility,
    /// Optional description for documentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional tags for categorization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl MemoryEntryData {
    /// Create a new memory entry with a string value.
    pub fn new(value: impl Into<MemoryValue>) -> Self {
        Self {
            value: value.into(),
            visibility: Visibility::default(),
            description: None,
            tags: Vec::new(),
        }
    }

    /// Set the visibility.
    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

/// A single memory entry with its key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryEntry {
    /// Unique key within the memory.yaml file.
    pub key: String,
    /// The entry data.
    #[serde(flatten)]
    pub data: MemoryEntryData,
}

impl MemoryEntry {
    /// Create a new memory entry.
    pub fn new(key: impl Into<String>, data: MemoryEntryData) -> Self {
        Self {
            key: key.into(),
            data,
        }
    }
}

/// Parsed memory.yaml file.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MemoryFile {
    /// Map of key -> MemoryEntryData.
    #[serde(flatten)]
    pub entries: IndexMap<String, MemoryEntryData>,
}

impl MemoryFile {
    /// Create a new empty memory file.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a memory.yaml file from string content.
    pub fn parse(content: &str) -> Result<Self, HmsError> {
        if content.trim().is_empty() {
            return Ok(Self::new());
        }
        serde_yaml::from_str(content).map_err(|e| HmsError::ParseError(e.to_string()))
    }

    /// Serialize to YAML string.
    pub fn to_yaml(&self) -> Result<String, HmsError> {
        serde_yaml::to_string(self).map_err(|e| HmsError::SerializeError(e.to_string()))
    }

    /// Get an entry by key.
    pub fn get(&self, key: &str) -> Option<&MemoryEntryData> {
        self.entries.get(key)
    }

    /// Insert or update an entry.
    pub fn set(&mut self, key: impl Into<String>, data: MemoryEntryData) {
        self.entries.insert(key.into(), data);
    }

    /// Remove an entry by key.
    pub fn remove(&mut self, key: &str) -> Option<MemoryEntryData> {
        self.entries.shift_remove(key)
    }

    /// Check if the file is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Iterate over entries.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &MemoryEntryData)> {
        self.entries.iter()
    }
}

/// A resolved memory entry with source information.
#[derive(Debug, Clone)]
pub struct ResolvedMemory {
    /// The entry key.
    pub key: String,
    /// The memory value.
    pub value: MemoryValue,
    /// Visibility scope.
    pub visibility: Visibility,
    /// Path to the source memory.yaml file.
    pub source_path: PathBuf,
    /// Storage class of the source.
    pub storage_class: StorageClass,
    /// Optional description.
    pub description: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
}

impl ResolvedMemory {
    /// Create from a key, entry data, and source information.
    pub fn from_entry(
        key: String,
        data: &MemoryEntryData,
        source_path: PathBuf,
        storage_class: StorageClass,
    ) -> Self {
        Self {
            key,
            value: data.value.clone(),
            visibility: data.visibility,
            source_path,
            storage_class,
            description: data.description.clone(),
            tags: data.tags.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visibility_display() {
        assert_eq!(Visibility::Public.to_string(), "public");
        assert_eq!(Visibility::Upstream.to_string(), "upstream");
        assert_eq!(Visibility::Downstream.to_string(), "downstream");
        assert_eq!(Visibility::Private.to_string(), "private");
    }

    #[test]
    fn test_visibility_from_str() {
        assert_eq!(Visibility::from_str("public").unwrap(), Visibility::Public);
        assert_eq!(
            Visibility::from_str("UPSTREAM").unwrap(),
            Visibility::Upstream
        );
        assert!(Visibility::from_str("invalid").is_err());
    }

    #[test]
    fn test_storage_class_display() {
        assert_eq!(StorageClass::Tracked.to_string(), "tracked");
        assert_eq!(StorageClass::Local.to_string(), "local");
        assert_eq!(StorageClass::User.to_string(), "user");
    }

    #[test]
    fn test_storage_class_from_str() {
        assert_eq!(
            StorageClass::from_str("tracked").unwrap(),
            StorageClass::Tracked
        );
        assert_eq!(StorageClass::from_str("LOCAL").unwrap(), StorageClass::Local);
        assert!(StorageClass::from_str("invalid").is_err());
    }

    #[test]
    fn test_storage_class_memory_path() {
        let base = PathBuf::from("/project");
        assert_eq!(
            StorageClass::Tracked.memory_path(&base),
            PathBuf::from("/project/.wonopcode/memory.yaml")
        );
        assert_eq!(
            StorageClass::Local.memory_path(&base),
            PathBuf::from("/project/.wonopcode-local/memory.yaml")
        );
        // User path depends on system, just check it ends correctly
        assert!(StorageClass::User
            .memory_path(&base)
            .ends_with("wonopcode/memory.yaml"));
    }

    #[test]
    fn test_memory_value_string() {
        let value = MemoryValue::String("test".to_string());
        assert_eq!(value.to_string_lossy(), "test");
        assert!(!value.is_empty());
    }

    #[test]
    fn test_memory_value_list() {
        let value = MemoryValue::List(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(value.to_string_lossy(), "a\nb");
        assert!(!value.is_empty());
    }

    #[test]
    fn test_memory_entry_data_builder() {
        let data = MemoryEntryData::new("value")
            .with_visibility(Visibility::Private)
            .with_description("desc")
            .with_tags(vec!["tag1".to_string()]);

        assert_eq!(data.value, MemoryValue::String("value".to_string()));
        assert_eq!(data.visibility, Visibility::Private);
        assert_eq!(data.description, Some("desc".to_string()));
        assert_eq!(data.tags, vec!["tag1".to_string()]);
    }

    #[test]
    fn test_memory_file_parse_string_value() {
        let yaml = r#"
project_name:
  value: "My Project"
  visibility: public
"#;
        let file = MemoryFile::parse(yaml).unwrap();
        assert_eq!(file.len(), 1);

        let entry = file.get("project_name").unwrap();
        assert_eq!(
            entry.value,
            MemoryValue::String("My Project".to_string())
        );
        assert_eq!(entry.visibility, Visibility::Public);
    }

    #[test]
    fn test_memory_file_parse_list_value() {
        let yaml = r#"
conventions:
  value:
    - "Use snake_case"
    - "Add doc comments"
  visibility: downstream
"#;
        let file = MemoryFile::parse(yaml).unwrap();
        let entry = file.get("conventions").unwrap();

        match &entry.value {
            MemoryValue::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], "Use snake_case");
            }
            _ => panic!("Expected list value"),
        }
        assert_eq!(entry.visibility, Visibility::Downstream);
    }

    #[test]
    fn test_memory_file_parse_map_value() {
        let yaml = r#"
api_config:
  value:
    style: REST
    version: v1
  visibility: upstream
"#;
        let file = MemoryFile::parse(yaml).unwrap();
        let entry = file.get("api_config").unwrap();

        match &entry.value {
            MemoryValue::Map(map) => {
                assert!(map.contains_key("style"));
            }
            _ => panic!("Expected map value"),
        }
        assert_eq!(entry.visibility, Visibility::Upstream);
    }

    #[test]
    fn test_memory_file_parse_default_visibility() {
        let yaml = r#"
simple_key:
  value: "Simple value"
"#;
        let file = MemoryFile::parse(yaml).unwrap();
        let entry = file.get("simple_key").unwrap();
        assert_eq!(entry.visibility, Visibility::Public); // default
    }

    #[test]
    fn test_memory_file_parse_with_optional_fields() {
        let yaml = r#"
documented_key:
  value: "Value"
  description: "This is a description"
  tags:
    - architecture
    - important
"#;
        let file = MemoryFile::parse(yaml).unwrap();
        let entry = file.get("documented_key").unwrap();

        assert_eq!(entry.description, Some("This is a description".to_string()));
        assert_eq!(
            entry.tags,
            vec!["architecture".to_string(), "important".to_string()]
        );
    }

    #[test]
    fn test_memory_file_parse_empty() {
        let file = MemoryFile::parse("").unwrap();
        assert!(file.is_empty());
    }

    #[test]
    fn test_memory_file_parse_invalid() {
        let result = MemoryFile::parse("invalid: [unclosed");
        assert!(result.is_err());
    }

    #[test]
    fn test_memory_file_roundtrip() {
        let mut file = MemoryFile::new();
        file.set(
            "key1",
            MemoryEntryData::new("value1").with_visibility(Visibility::Private),
        );
        file.set("key2", MemoryEntryData::new("value2"));

        let yaml = file.to_yaml().unwrap();
        let parsed = MemoryFile::parse(&yaml).unwrap();

        assert_eq!(file, parsed);
    }

    #[test]
    fn test_memory_file_operations() {
        let mut file = MemoryFile::new();
        assert!(file.is_empty());

        file.set("key1", MemoryEntryData::new("value1"));
        assert_eq!(file.len(), 1);
        assert!(file.get("key1").is_some());

        let removed = file.remove("key1");
        assert!(removed.is_some());
        assert!(file.is_empty());
    }
}
