//! ES-compatible API endpoints

pub mod bulk;
pub mod cluster;
pub mod cluster_extra;
pub mod document;
pub mod ilm;
pub mod mapping;
pub mod msearch;
pub mod search;
pub mod tasks;
pub mod templates;

pub use bulk::bulk_handler;
pub use cluster::{cat_indices_handler, cluster_health_handler, cluster_health_index_handler, root_handler};
pub use cluster_extra::{cat_aliases_handler, cat_nodes_handler, cat_shards_handler, cluster_settings_handler, cluster_state_handler, license_handler, nodes_handler, nodes_stats_handler, update_aliases_handler, xpack_handler};
pub use ilm::xpack_usage_handler;
pub use document::{
    count_handler, delete_doc_handler, get_doc_handler, get_index_handler, get_search_handler, head_doc_handler,
    head_index_handler, post_doc_handler, create_doc_handler, put_doc_handler, put_index_handler,
};
pub use mapping::{mapping_handler, put_mapping_handler};
pub use msearch::msearch_handler;
pub use search::{create_pit_handler, delete_pit_handler, search_handler};
pub use tasks::{delete_by_query_handler, get_task_handler, update_by_query_handler};
pub use templates::{
    delete_component_template_handler, delete_index_template_handler,
    get_all_index_templates_handler, get_component_template_handler, get_index_template_handler, head_index_template_handler,
    put_component_template_handler, put_index_template_handler,
};
