#[cfg(feature = "vector-instant")]
use searchcore::backends::vector::{HnswBackend, HnswIndex, Metric};

#[cfg(feature = "vector-instant")]
#[test]
fn test_instant_distance_adapter() {
    let mut index = HnswBackend::new(3, Metric::Cosine, 16, 200).unwrap();

    // Add vectors
    index.add(0, &[1.0, 0.0, 0.0]).unwrap();
    index.add(1, &[0.0, 1.0, 0.0]).unwrap();
    index.add(2, &[0.9, 0.1, 0.0]).unwrap();

    // Search
    let results = index.search(&[0.95, 0.05, 0.0], 2, 100).unwrap();

    assert_eq!(results.len(), 2);
    // Should find key 0 and 2 (closest to query)
    assert!(results.iter().any(|(k, _)| *k == 0));
    assert!(results.iter().any(|(k, _)| *k == 2));
}
