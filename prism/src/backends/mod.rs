pub mod text;
pub mod r#trait;
pub mod vector;
pub mod hybrid;

pub use r#trait::{BackendStats, Document, Query, SearchBackend, SearchResult, SearchResults, SearchResultsWithAggs};
pub use text::TextBackend;
pub use vector::VectorBackend;
pub use hybrid::HybridSearchCoordinator;
