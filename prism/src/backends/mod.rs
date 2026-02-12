pub mod graph;
pub mod hybrid;
pub mod text;
pub mod r#trait;
pub mod vector;

pub use graph::{GraphEdge, GraphNode, GraphStats, ShardedGraphBackend};
pub use hybrid::HybridSearchCoordinator;
pub use r#trait::{
    BackendStats, Document, HighlightConfig, Query, SearchBackend, SearchResult, SearchResults,
    SearchResultsWithAggs,
};
pub use text::TextBackend;
pub use vector::VectorBackend;
