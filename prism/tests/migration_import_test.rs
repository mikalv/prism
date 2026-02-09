use prism::migration::import::DataImporter;
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_import_creates_importer() {
    let temp_dir = TempDir::new().unwrap();
    let input_dir = temp_dir.path().join("input");
    fs::create_dir(&input_dir).unwrap();

    let _importer = DataImporter::new(input_dir, "http://localhost:8080".to_string());

    // Just verify it constructs successfully
    assert!(true);
}

#[tokio::test]
async fn test_import_reads_jsonl() {
    let temp_dir = TempDir::new().unwrap();
    let input_dir = temp_dir.path().join("input");
    fs::create_dir(&input_dir).unwrap();

    // Create a test JSONL file
    let test_file = input_dir.join("test_collection.jsonl");
    let test_data = r#"{"id":"1","title":"Test Doc 1","count":42}
{"id":"2","title":"Test Doc 2","count":100}
"#;
    fs::write(&test_file, test_data).unwrap();

    let importer = DataImporter::new(input_dir.clone(), "http://localhost:8080".to_string());
    let docs = importer.read_jsonl_file(&test_file).await.unwrap();

    assert_eq!(docs.len(), 2);
    assert_eq!(docs[0].get("id").unwrap().as_str().unwrap(), "1");
    assert_eq!(docs[1].get("count").unwrap().as_i64().unwrap(), 100);
}
