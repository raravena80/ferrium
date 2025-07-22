use super::*;
use openraft::{EntryPayload, Membership};
use tempfile::TempDir;

/// Helper function to create a temporary storage instance
async fn create_test_storage() -> (FerriiteStorage, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let storage = FerriiteStorage::new(temp_dir.path().join("test.db"))
        .await
        .expect("Failed to create test storage");
    (storage, temp_dir)
}

/// Helper function to create a CommittedLeaderId from term and leader_id
fn create_leader_id(term: u64, leader_id: NodeId) -> CommittedLeaderId<NodeId> {
    CommittedLeaderId::new(term, leader_id)
}

/// Helper function to create a test log entry
fn create_test_entry(index: u64, term: u64, request: KvRequest) -> Entry<TypeConfig> {
    Entry {
        log_id: LogId::new(create_leader_id(term, 1), index),
        payload: EntryPayload::Normal(request),
    }
}

#[tokio::test]
async fn test_storage_creation() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().join("test.db");

    // Test creating new storage
    let storage = FerriiteStorage::new(&db_path).await;
    assert!(storage.is_ok());

    let storage = storage.unwrap();
    assert!(storage.state_machine.data.is_empty());
    assert!(storage.state_machine.last_applied_log_id.is_none());

    // Test that database files are created
    assert!(db_path.exists());
}

#[tokio::test]
async fn test_storage_reopen() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().join("test.db");

    // Create storage and add some data
    {
        let mut storage = FerriiteStorage::new(&db_path).await.unwrap();
        storage
            .state_machine
            .data
            .insert("key1".to_string(), "value1".to_string());

        // Persist the snapshot
        let mut snapshot_builder = storage.get_snapshot_builder().await;
        let _snapshot = snapshot_builder.build_snapshot().await.unwrap();
    }

    // Reopen storage and verify data is still there
    let storage = FerriiteStorage::new(&db_path).await.unwrap();
    assert_eq!(
        storage.state_machine.data.get("key1"),
        Some(&"value1".to_string())
    );
}

#[tokio::test]
async fn test_log_entries_storage() {
    let (mut storage, _temp_dir) = create_test_storage().await;

    // Test appending log entries
    let entries = vec![
        create_test_entry(
            1,
            1,
            KvRequest::Set {
                key: "key1".to_string(),
                value: "value1".to_string(),
            },
        ),
        create_test_entry(
            2,
            1,
            KvRequest::Set {
                key: "key2".to_string(),
                value: "value2".to_string(),
            },
        ),
        create_test_entry(
            3,
            2,
            KvRequest::Get {
                key: "key1".to_string(),
            },
        ),
    ];

    storage
        .append_to_log(entries.clone())
        .await
        .expect("Failed to append entries");

    // Test retrieving log entries
    let retrieved = storage
        .try_get_log_entries(1..=3)
        .await
        .expect("Failed to get log entries");
    assert_eq!(retrieved.len(), 3);
    assert_eq!(retrieved[0].log_id.index, 1);
    assert_eq!(retrieved[1].log_id.index, 2);
    assert_eq!(retrieved[2].log_id.index, 3);
    assert_eq!(retrieved[2].log_id.leader_id.term, 2);
}

#[tokio::test]
async fn test_log_entries_range_queries() {
    let (mut storage, _temp_dir) = create_test_storage().await;

    // Add several log entries
    let entries: Vec<_> = (1..=10)
        .map(|i| {
            create_test_entry(
                i,
                1,
                KvRequest::Set {
                    key: format!("key{}", i),
                    value: format!("value{}", i),
                },
            )
        })
        .collect();

    storage
        .append_to_log(entries)
        .await
        .expect("Failed to append entries");

    // Test various range queries
    let range_1_5 = storage
        .try_get_log_entries(1..=5)
        .await
        .expect("Failed to get range 1..=5");
    assert_eq!(range_1_5.len(), 5);
    assert_eq!(range_1_5.first().unwrap().log_id.index, 1);
    assert_eq!(range_1_5.last().unwrap().log_id.index, 5);

    let range_6_10 = storage
        .try_get_log_entries(6..=10)
        .await
        .expect("Failed to get range 6..=10");
    assert_eq!(range_6_10.len(), 5);
    assert_eq!(range_6_10.first().unwrap().log_id.index, 6);
    assert_eq!(range_6_10.last().unwrap().log_id.index, 10);

    // Test empty range
    let empty_range = storage
        .try_get_log_entries(20..=25)
        .await
        .expect("Failed to get empty range");
    assert_eq!(empty_range.len(), 0);
}

#[tokio::test]
async fn test_log_deletion() {
    let (mut storage, _temp_dir) = create_test_storage().await;

    // Add log entries
    let entries: Vec<_> = (1..=10)
        .map(|i| {
            create_test_entry(
                i,
                1,
                KvRequest::Set {
                    key: format!("key{}", i),
                    value: format!("value{}", i),
                },
            )
        })
        .collect();

    storage
        .append_to_log(entries)
        .await
        .expect("Failed to append entries");

    // Test conflict log deletion (delete from index 6 onwards)
    let conflict_log_id = LogId::new(create_leader_id(1, 1), 6);
    storage
        .delete_conflict_logs_since(conflict_log_id)
        .await
        .expect("Failed to delete conflict logs");

    // Verify only entries 1-5 remain
    let remaining = storage
        .try_get_log_entries(1..=10)
        .await
        .expect("Failed to get remaining entries");
    assert_eq!(remaining.len(), 5);
    assert_eq!(remaining.last().unwrap().log_id.index, 5);

    // Test log purging (purge up to index 3)
    let purge_log_id = LogId::new(create_leader_id(1, 1), 3);
    storage
        .purge_logs_upto(purge_log_id)
        .await
        .expect("Failed to purge logs");

    // Verify entries 4-5 remain, and last_purged is set
    let after_purge = storage
        .try_get_log_entries(1..=10)
        .await
        .expect("Failed to get entries after purge");
    assert_eq!(after_purge.len(), 2);
    assert_eq!(after_purge.first().unwrap().log_id.index, 4);

    let log_state = storage
        .get_log_state()
        .await
        .expect("Failed to get log state");
    assert_eq!(log_state.last_purged_log_id, Some(purge_log_id));
    assert_eq!(
        log_state.last_log_id,
        Some(LogId::new(create_leader_id(1, 1), 5))
    );
}

#[tokio::test]
async fn test_state_machine_operations() {
    let (mut storage, _temp_dir) = create_test_storage().await;

    // Test applying entries to state machine
    let entries = vec![
        create_test_entry(
            1,
            1,
            KvRequest::Set {
                key: "name".to_string(),
                value: "ferrite".to_string(),
            },
        ),
        create_test_entry(
            2,
            1,
            KvRequest::Set {
                key: "version".to_string(),
                value: "1.0".to_string(),
            },
        ),
        create_test_entry(
            3,
            1,
            KvRequest::Get {
                key: "name".to_string(),
            },
        ),
        create_test_entry(
            4,
            1,
            KvRequest::Delete {
                key: "version".to_string(),
            },
        ),
    ];

    let responses = storage
        .apply_to_state_machine(&entries)
        .await
        .expect("Failed to apply to state machine");

    // Verify responses
    assert_eq!(responses.len(), 4);
    assert!(matches!(responses[0], KvResponse::Set));
    assert!(matches!(responses[1], KvResponse::Set));
    assert!(matches!(responses[2], KvResponse::Get { value: Some(ref v) } if v == "ferrite"));
    assert!(matches!(responses[3], KvResponse::Delete));

    // Verify state machine state
    assert_eq!(
        storage.state_machine.data.get("name"),
        Some(&"ferrite".to_string())
    );
    assert_eq!(storage.state_machine.data.get("version"), None); // Should be deleted
    assert_eq!(
        storage.state_machine.last_applied_log_id,
        Some(LogId::new(create_leader_id(1, 1), 4))
    );
}

#[tokio::test]
async fn test_vote_persistence() {
    let (mut storage, _temp_dir) = create_test_storage().await;

    // Test saving and reading votes
    let vote = Vote::new(5, 1);
    storage.save_vote(&vote).await.expect("Failed to save vote");

    let read_vote = storage.read_vote().await.expect("Failed to read vote");
    assert_eq!(read_vote, Some(vote));

    // Test overwriting vote
    let new_vote = Vote::new(7, 2);
    storage
        .save_vote(&new_vote)
        .await
        .expect("Failed to save new vote");

    let read_new_vote = storage.read_vote().await.expect("Failed to read new vote");
    assert_eq!(read_new_vote, Some(new_vote));
}

#[tokio::test]
async fn test_snapshot_creation_and_installation() {
    let (mut storage, _temp_dir) = create_test_storage().await;

    // Add some data to state machine
    let entries = vec![
        create_test_entry(
            1,
            1,
            KvRequest::Set {
                key: "config".to_string(),
                value: "production".to_string(),
            },
        ),
        create_test_entry(
            2,
            1,
            KvRequest::Set {
                key: "nodes".to_string(),
                value: "3".to_string(),
            },
        ),
    ];

    storage
        .apply_to_state_machine(&entries)
        .await
        .expect("Failed to apply entries");

    // Create a snapshot
    let mut snapshot_builder = storage.get_snapshot_builder().await;
    let snapshot = snapshot_builder
        .build_snapshot()
        .await
        .expect("Failed to build snapshot");

    // Verify snapshot metadata
    assert_eq!(
        snapshot.meta.last_log_id,
        Some(LogId::new(create_leader_id(1, 1), 2))
    );

    // Verify snapshot data
    let snapshot_data: KvSnapshot = serde_json::from_slice(snapshot.snapshot.get_ref())
        .expect("Failed to deserialize snapshot data");
    assert_eq!(
        snapshot_data.data.get("config"),
        Some(&"production".to_string())
    );
    assert_eq!(snapshot_data.data.get("nodes"), Some(&"3".to_string()));

    // Test installing the snapshot on a fresh storage
    let (mut fresh_storage, _temp_dir2) = create_test_storage().await;
    fresh_storage
        .install_snapshot(&snapshot.meta, snapshot.snapshot)
        .await
        .expect("Failed to install snapshot");

    // Verify fresh storage has the snapshot data
    assert_eq!(
        fresh_storage.state_machine.data.get("config"),
        Some(&"production".to_string())
    );
    assert_eq!(
        fresh_storage.state_machine.data.get("nodes"),
        Some(&"3".to_string())
    );
    assert_eq!(
        fresh_storage.state_machine.last_applied_log_id,
        Some(LogId::new(create_leader_id(1, 1), 2))
    );
}

#[tokio::test]
async fn test_snapshot_persistence_across_restarts() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().join("test.db");

    let snapshot_meta = {
        // Create storage and add data
        let mut storage = FerriiteStorage::new(&db_path).await.unwrap();
        storage
            .state_machine
            .data
            .insert("persistent".to_string(), "data".to_string());
        storage.state_machine.last_applied_log_id = Some(LogId::new(create_leader_id(2, 1), 5));

        // Create and save snapshot
        let mut snapshot_builder = storage.get_snapshot_builder().await;
        let snapshot = snapshot_builder.build_snapshot().await.unwrap();
        snapshot.meta.clone()
    };

    // Reopen storage and verify snapshot is restored
    let mut reopened_storage = FerriiteStorage::new(&db_path).await.unwrap();
    let current_snapshot = reopened_storage.get_current_snapshot().await.unwrap();

    assert!(current_snapshot.is_some());
    let snapshot = current_snapshot.unwrap();
    assert_eq!(snapshot.meta.last_log_id, snapshot_meta.last_log_id);

    // Verify data is restored
    assert_eq!(
        reopened_storage.state_machine.data.get("persistent"),
        Some(&"data".to_string())
    );
}

#[tokio::test]
async fn test_membership_changes() {
    let (mut storage, _temp_dir) = create_test_storage().await;

    // Create a membership change entry
    let mut voters = BTreeSet::new();
    voters.insert(1);
    voters.insert(2);
    voters.insert(3);
    let membership = Membership::new(vec![voters], None);

    let membership_entry = Entry {
        log_id: LogId::new(create_leader_id(1, 1), 1),
        payload: EntryPayload::Membership(membership.clone()),
    };

    let responses = storage
        .apply_to_state_machine(&[membership_entry])
        .await
        .expect("Failed to apply membership change");

    assert_eq!(responses.len(), 1);
    assert!(matches!(responses[0], KvResponse::Set));

    // Verify membership is stored
    let (last_applied, last_membership) = storage
        .last_applied_state()
        .await
        .expect("Failed to get last applied state");

    assert_eq!(last_applied, Some(LogId::new(create_leader_id(1, 1), 1)));
    assert_eq!(last_membership.membership(), &membership);
}

#[tokio::test]
async fn test_concurrent_storage_cloning() {
    let (storage, _temp_dir) = create_test_storage().await;

    // Test that storage can be cloned (shares the same database)
    let cloned_storage = storage.clone();

    // Both storages should point to the same database
    assert!(std::ptr::eq(
        storage.db.as_ref(),
        cloned_storage.db.as_ref()
    ));

    // State machines should be independent copies
    assert_eq!(
        storage.state_machine.data,
        cloned_storage.state_machine.data
    );
}

#[tokio::test]
async fn test_error_handling() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().join("test.db");

    // Create storage successfully
    let mut storage = FerriiteStorage::new(&db_path).await.unwrap();

    // Test invalid snapshot data handling
    let invalid_snapshot = Box::new(Cursor::new(b"invalid json data".to_vec()));
    let mut voters = BTreeSet::new();
    voters.insert(1);
    let dummy_meta = SnapshotMeta {
        last_log_id: Some(LogId::new(create_leader_id(1, 1), 1)),
        last_membership: StoredMembership::new(None, Membership::new(vec![voters], None)),
        snapshot_id: "test".to_string(),
    };

    let result = storage
        .install_snapshot(&dummy_meta, invalid_snapshot)
        .await;
    assert!(result.is_err());

    // Test that storage is still functional after error
    let vote = Vote::new(1, 1);
    storage
        .save_vote(&vote)
        .await
        .expect("Storage should still work after error");
}

#[tokio::test]
async fn test_new_storage_factory() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().join("test.db");

    // Test the factory function
    let result = new_storage(&db_path).await;
    assert!(result.is_ok());

    let (log_store, state_machine_store) = result.unwrap();

    // Both stores should be adapters around the same underlying storage
    // We can't directly test this, but we can test that they work
    let mut log_store_mut = log_store;
    let mut state_machine_store_mut = state_machine_store;
    assert!(log_store_mut.get_log_state().await.is_ok());
    assert!(state_machine_store_mut.get_log_state().await.is_ok());
}

#[tokio::test]
async fn test_large_dataset_performance() {
    let (mut storage, _temp_dir) = create_test_storage().await;

    // Test with a larger dataset to ensure reasonable performance
    let entries: Vec<_> = (1..=1000)
        .map(|i| {
            create_test_entry(
                i,
                1,
                KvRequest::Set {
                    key: format!("performance_key_{}", i),
                    value: format!("performance_value_{}", i),
                },
            )
        })
        .collect();

    // This should complete in reasonable time
    let start = std::time::Instant::now();
    storage
        .append_to_log(entries.clone())
        .await
        .expect("Failed to append large dataset");
    let append_time = start.elapsed();

    // Apply to state machine
    let start = std::time::Instant::now();
    let _responses = storage
        .apply_to_state_machine(&entries)
        .await
        .expect("Failed to apply large dataset");
    let apply_time = start.elapsed();

    // Verify some entries were stored correctly
    let retrieved = storage
        .try_get_log_entries(1..=10)
        .await
        .expect("Failed to retrieve entries");
    assert_eq!(retrieved.len(), 10);
    assert_eq!(storage.state_machine.data.len(), 1000);

    // Log performance info (not assertions, just for visibility)
    println!("Large dataset performance:");
    println!("  Append 1000 entries: {:?}", append_time);
    println!("  Apply 1000 entries: {:?}", apply_time);
    println!(
        "  Final state machine size: {}",
        storage.state_machine.data.len()
    );

    // Basic performance checks (generous limits)
    assert!(
        append_time < std::time::Duration::from_secs(5),
        "Append took too long: {:?}",
        append_time
    );
    assert!(
        apply_time < std::time::Duration::from_secs(5),
        "Apply took too long: {:?}",
        apply_time
    );
}

#[tokio::test]
async fn test_binary_key_conversion_helpers() {
    // Test the helper functions for key conversion
    let test_values = vec![0u64, 1, 42, 1000, u64::MAX];

    for value in test_values {
        let binary = id_to_bin(value);
        let recovered = bin_to_id(&binary);
        assert_eq!(
            value, recovered,
            "Failed to convert value {} correctly",
            value
        );
    }

    // Test that binary keys sort correctly
    let values = vec![1u64, 5, 10, 42, 100];
    let mut binary_keys: Vec<_> = values.iter().map(|v| id_to_bin(*v)).collect();
    binary_keys.sort();

    let recovered_values: Vec<_> = binary_keys.iter().map(|k| bin_to_id(k)).collect();
    assert_eq!(values, recovered_values, "Binary keys don't sort correctly");
}
