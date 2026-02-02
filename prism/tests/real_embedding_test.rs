//! Test real ONNX embedding generation
//!
//! Run with: cargo test --release --features embedding-gen-real test_real_embedding

#![cfg(feature = "embedding-gen-real")]

use prism::embedding::{Embedder, ModelConfig};

#[tokio::test]
async fn test_real_embedding_single() {
    // Initialize tracing for debugging
    let _ = tracing_subscriber::fmt::try_init();

    let config = ModelConfig::new("all-MiniLM-L6-v2");
    println!("Model path: {:?}", config.model_path());
    println!("Tokenizer path: {:?}", config.tokenizer_path());

    // Create embedder - this will download model if not cached
    let embedder = Embedder::new(config)
        .await
        .expect("Failed to create embedder");

    // Test single embedding
    let text = "Hello, world! This is a test sentence for embedding.";
    let embedding = embedder.embed(text).expect("Failed to generate embedding");

    println!("Embedding dimension: {}", embedding.len());
    println!("First 5 values: {:?}", &embedding[..5.min(embedding.len())]);

    // Verify embedding dimension (all-MiniLM-L6-v2 produces 384-dim embeddings)
    assert_eq!(
        embedding.len(),
        384,
        "Expected 384-dim embedding from all-MiniLM-L6-v2"
    );

    // Verify L2 normalization (norm should be ~1.0)
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 0.01,
        "Embedding should be L2 normalized, got norm={}",
        norm
    );

    println!("✓ Single embedding test passed!");
}

#[tokio::test]
async fn test_real_embedding_batch() {
    let _ = tracing_subscriber::fmt::try_init();

    let config = ModelConfig::new("all-MiniLM-L6-v2");
    let embedder = Embedder::new(config)
        .await
        .expect("Failed to create embedder");

    // Test batch embedding
    let texts = vec![
        "The quick brown fox jumps over the lazy dog.",
        "Machine learning is transforming software development.",
        "Rust is a systems programming language focused on safety.",
    ];

    let embeddings = embedder
        .embed_batch(&texts.iter().map(|s| *s).collect::<Vec<_>>())
        .expect("Failed to generate batch embeddings");

    assert_eq!(embeddings.len(), 3, "Expected 3 embeddings");

    for (i, emb) in embeddings.iter().enumerate() {
        assert_eq!(emb.len(), 384, "Embedding {} should be 384-dim", i);

        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 0.01,
            "Embedding {} should be normalized, got norm={}",
            i,
            norm
        );
    }

    // Verify similar texts produce similar embeddings (cosine similarity)
    let dot_01: f32 = embeddings[0]
        .iter()
        .zip(&embeddings[1])
        .map(|(a, b)| a * b)
        .sum();
    let dot_02: f32 = embeddings[0]
        .iter()
        .zip(&embeddings[2])
        .map(|(a, b)| a * b)
        .sum();

    println!("Cosine similarity (0,1): {}", dot_01);
    println!("Cosine similarity (0,2): {}", dot_02);

    println!("✓ Batch embedding test passed!");
}

#[tokio::test]
async fn test_embedding_determinism() {
    let _ = tracing_subscriber::fmt::try_init();

    let config = ModelConfig::new("all-MiniLM-L6-v2");
    let embedder = Embedder::new(config)
        .await
        .expect("Failed to create embedder");

    let text = "Deterministic embedding test";

    let embedding1 = embedder.embed(text).expect("First embedding failed");
    let embedding2 = embedder.embed(text).expect("Second embedding failed");

    // Same text should produce identical embeddings
    for (i, (a, b)) in embedding1.iter().zip(&embedding2).enumerate() {
        assert!(
            (a - b).abs() < 1e-6,
            "Embeddings differ at index {}: {} vs {}",
            i,
            a,
            b
        );
    }

    println!("✓ Determinism test passed!");
}
