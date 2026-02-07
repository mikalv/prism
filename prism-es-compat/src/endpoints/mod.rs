//! ES-compatible API endpoints

pub mod bulk;
pub mod cluster;
pub mod mapping;
pub mod msearch;
pub mod search;

pub use bulk::bulk_handler;
pub use cluster::{cat_indices_handler, cluster_health_handler, root_handler};
pub use mapping::mapping_handler;
pub use msearch::msearch_handler;
pub use search::search_handler;
