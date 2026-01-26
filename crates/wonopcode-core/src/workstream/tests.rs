//! Tests for workstream module.

use super::*;

#[test]
fn test_workstream_id_from_branch() {
    let id = WorkstreamId::from_branch("feature/my-feature");
    assert_eq!(id.branch(), "feature/my-feature");
    assert_eq!(id.display_name(), "my-feature");
}

#[test]
fn test_workstream_id_display_name_strips_prefixes() {
    assert_eq!(
        WorkstreamId::from_branch("feature/test").display_name(),
        "test"
    );
    assert_eq!(
        WorkstreamId::from_branch("bugfix/fix-bug").display_name(),
        "fix-bug"
    );
    assert_eq!(
        WorkstreamId::from_branch("hotfix/urgent").display_name(),
        "urgent"
    );
    assert_eq!(
        WorkstreamId::from_branch("refs/heads/main").display_name(),
        "main"
    );
    assert_eq!(
        WorkstreamId::from_branch("main").display_name(),
        "main"
    );
}

#[test]
fn test_workstream_id_equality() {
    let id1 = WorkstreamId::from_branch("feature/test");
    let id2 = WorkstreamId::from_branch("feature/test");
    let id3 = WorkstreamId::from_branch("feature/other");
    
    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_workstream_id_hash() {
    use std::collections::HashSet;
    
    let mut set = HashSet::new();
    set.insert(WorkstreamId::from_branch("feature/test"));
    set.insert(WorkstreamId::from_branch("feature/test"));
    set.insert(WorkstreamId::from_branch("feature/other"));
    
    assert_eq!(set.len(), 2);
}

#[test]
fn test_passive_workstream_to_info() {
    let passive = PassiveWorkstream {
        branch: "feature/test".to_string(),
        path: std::path::PathBuf::from("/tmp/test"),
        is_direct: false,
        repo_root: std::path::PathBuf::from("/tmp/repo"),
        discovered_at: std::time::SystemTime::now(),
    };
    
    let info = passive.to_info();
    
    assert_eq!(info.id.branch(), "feature/test");
    assert_eq!(info.name, "feature/test");
    assert!(!info.is_active);
    assert!(!info.is_direct);
    assert_eq!(info.status, WorkstreamStatus::Passive);
    assert_eq!(info.connected_clients, 0);
}

#[test]
fn test_passive_workstream_direct() {
    let passive = PassiveWorkstream {
        branch: "main".to_string(),
        path: std::path::PathBuf::from("/tmp/repo"),
        is_direct: true,
        repo_root: std::path::PathBuf::from("/tmp/repo"),
        discovered_at: std::time::SystemTime::now(),
    };
    
    let info = passive.to_info();
    
    assert!(info.is_direct);
    assert!(!info.is_active);
}

#[test]
fn test_workstream_status_display() {
    assert_eq!(format!("{}", WorkstreamStatus::Passive), "Passive");
    assert_eq!(format!("{}", WorkstreamStatus::Active), "Active");
    assert_eq!(format!("{}", WorkstreamStatus::Activating), "Activating");
    assert_eq!(format!("{}", WorkstreamStatus::Deactivating), "Deactivating");
    assert_eq!(format!("{}", WorkstreamStatus::Error), "Error");
}

#[test]
fn test_workstream_event_serialization() {
    let event = WorkstreamEvent::Activated {
        id: WorkstreamId::from_branch("feature/test"),
    };
    
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("activated"));
    assert!(json.contains("feature/test"));
    
    let deserialized: WorkstreamEvent = serde_json::from_str(&json).unwrap();
    match deserialized {
        WorkstreamEvent::Activated { id } => {
            assert_eq!(id.branch(), "feature/test");
        }
        _ => panic!("Expected Activated event"),
    }
}

#[test]
fn test_workstream_info_serialization() {
    let info = WorkstreamInfo {
        id: WorkstreamId::from_branch("feature/test"),
        name: "feature/test".to_string(),
        description: Some("A test workstream".to_string()),
        worktree_path: std::path::PathBuf::from("/tmp/test"),
        status: WorkstreamStatus::Active,
        is_active: true,
        is_direct: false,
        created_at: std::time::SystemTime::UNIX_EPOCH,
        last_activity: std::time::SystemTime::UNIX_EPOCH,
        connected_clients: 2,
    };
    
    let json = serde_json::to_string(&info).unwrap();
    let deserialized: WorkstreamInfo = serde_json::from_str(&json).unwrap();
    
    assert_eq!(deserialized.id.branch(), "feature/test");
    assert_eq!(deserialized.name, "feature/test");
    assert!(deserialized.is_active);
    assert_eq!(deserialized.connected_clients, 2);
}
