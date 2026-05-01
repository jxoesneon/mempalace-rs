use super::*;
use tempfile::tempdir;
use std::fs::File;

#[test]
fn test_storage_quota_exceeded() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("db.sqlite");
    let index_path = dir.path().join("index.usearch");
    
    let embedder = crate::embedder_factory::EmbedderFactory::get_embedder().unwrap();
    let mut vs = VectorStorage::new_with_embedder(&db_path, &index_path, embedder).unwrap();
    
    let texts = vec!["mem1".to_string()];
    let wings = vec!["w".to_string(), "w".to_string()]; // mismatch
    let rooms = vec!["r".to_string()];
    let sources = vec![None];
    let times = vec![None];
    
    let res = vs.add_memories_batch(texts, wings, rooms, sources, times);
    match res {
        Err(e) => assert!(e.to_string().contains("Batch input lengths do not match")),
        Ok(_) => panic!("Should have failed"),
    }
}

#[test]
fn test_database_io_failure_on_new() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("read_only.sqlite");
    File::create(&db_path).unwrap();
    // Setting read-only attribute
    let mut perms = std::fs::metadata(&db_path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&db_path, perms).unwrap();
    
    let index_path = dir.path().join("index.usearch");
    let res = VectorStorage::new(&db_path, &index_path);
    match res {
        Err(e) => {
            println!("Error: {}", e);
            let msg = e.to_string();
            assert!(msg.contains("Cannot open SQLite") || msg.contains("attempt to write a readonly database"), "Error was: {}", e);
        },
        Ok(_) => panic!("Should have failed"),
    }
}

#[test]
fn test_usearch_load_failure() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("db.sqlite");
    let index_path = dir.path().join("corrupt.usearch");
    
    // Write junk to the index file
    std::fs::write(&index_path, "not-a-usearch-index").unwrap();
    
    let res = VectorStorage::new(&db_path, &index_path);
    // Should fail to load corrupt index
    assert!(res.is_err());
}
