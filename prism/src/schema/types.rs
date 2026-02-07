use serde::{Deserialize, Serialize};

use crate::storage::StorageConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSchema {
    pub collection: String,
    #[serde(default)]
    pub description: Option<String>,
    pub backends: Backends,
    #[serde(default)]
    pub indexing: IndexingConfig,
    #[serde(default)]
    pub quota: QuotaConfig,
    #[serde(default)]
    pub embedding_generation: Option<EmbeddingGenerationConfig>,

    /// Faceted search configuration
    #[serde(default)]
    pub facets: Option<FacetConfig>,

    /// Boosting configuration
    #[serde(default)]
    pub boosting: Option<BoostingConfig>,

    /// Storage backend configuration (local or S3)
    #[serde(default)]
    pub storage: StorageConfig,

    /// System fields configuration (auto-indexed fields for ranking)
    #[serde(default)]
    pub system_fields: SystemFieldsConfig,

    /// Hybrid search configuration
    #[serde(default)]
    pub hybrid: Option<HybridConfig>,

    /// Replication configuration for distributed deployments
    #[serde(default)]
    pub replication: Option<ReplicationConfig>,

    /// ILM policy name for index lifecycle management
    #[serde(default)]
    pub ilm_policy: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Backends {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextBackendConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<VectorBackendConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphBackendConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBackendConfig {
    pub fields: Vec<TextField>,

    /// BM25 k1 parameter for term saturation (default: 1.2)
    /// Higher values increase the impact of term frequency
    #[serde(default)]
    pub bm25_k1: Option<f32>,

    /// BM25 b parameter for document length normalization (default: 0.75)
    /// 0 = no length normalization, 1 = full normalization
    #[serde(default)]
    pub bm25_b: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    #[serde(default)]
    pub stored: bool,
    #[serde(default)]
    pub indexed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    Text,
    String,
    I64,
    U64,
    F64,
    Bool,
    Date,
    Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorBackendConfig {
    pub embedding_field: String,
    pub dimension: usize,
    #[serde(default = "default_distance")]
    pub distance: VectorDistance,
    #[serde(default = "default_hnsw_m")]
    pub hnsw_m: usize,
    #[serde(default = "default_hnsw_ef_construction")]
    pub hnsw_ef_construction: usize,
    #[serde(default = "default_hnsw_ef_search")]
    pub hnsw_ef_search: usize,
    #[serde(default = "default_vector_weight")]
    pub vector_weight: f32,
}

fn default_vector_weight() -> f32 {
    0.5
}

fn default_distance() -> VectorDistance {
    VectorDistance::Cosine
}

fn default_hnsw_m() -> usize {
    16
}

fn default_hnsw_ef_construction() -> usize {
    200
}

fn default_hnsw_ef_search() -> usize {
    100
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorDistance {
    Cosine,
    Euclidean,
    Dot,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexingConfig {
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_commit_interval_secs")]
    pub commit_interval_secs: u64,
    #[serde(default = "default_worker_threads")]
    pub worker_threads: usize,
}

fn default_batch_size() -> usize {
    1000
}

fn default_commit_interval_secs() -> u64 {
    5
}

fn default_worker_threads() -> usize {
    4
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QuotaConfig {
    #[serde(default)]
    pub max_documents: Option<usize>,
    #[serde(default)]
    pub max_size_mb: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingGenerationConfig {
    pub enabled: bool,
    pub model: String,
    pub source_field: String,
    pub target_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacetConfig {
    /// Fields allowed to be faceted
    #[serde(default)]
    pub allowed: Vec<String>,

    /// Default facets if not specified in query
    #[serde(default)]
    pub default: Vec<String>,

    /// Per-field facet configuration
    #[serde(default)]
    pub configs: Vec<FieldFacetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldFacetConfig {
    pub field: String,

    #[serde(rename = "type")]
    pub facet_type: FacetType,

    /// Max values to return (for terms aggregation)
    #[serde(default = "default_facet_size")]
    pub size: usize,

    /// Interval for date_histogram (e.g., "day", "week", "month")
    #[serde(default)]
    pub interval: Option<String>,
}

fn default_facet_size() -> usize {
    10
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacetType {
    Terms,
    DateHistogram,
    Range,
    Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoostingConfig {
    /// Recency decay configuration
    #[serde(default)]
    pub recency: Option<RecencyDecayConfig>,

    /// Context-based boosting (project, session, etc.)
    #[serde(default)]
    pub context: Vec<ContextBoostConfig>,

    /// Field-specific weights
    #[serde(default)]
    pub field_weights: std::collections::HashMap<String, f32>,

    /// Custom ranking signals: named numeric fields with scoring weights.
    /// Each signal references a stored f64/i64/u64 field in the document
    /// and contributes `field_value * weight` to the final score.
    #[serde(default)]
    pub signals: Vec<RankingSignal>,
}

/// A custom ranking signal that maps a document field to a scoring weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingSignal {
    /// Name of the document field (must be a stored numeric field)
    pub name: String,
    /// Weight multiplier for this signal's contribution to the score
    #[serde(default = "default_signal_weight")]
    pub weight: f32,
}

fn default_signal_weight() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecencyDecayConfig {
    /// Field containing timestamp
    pub field: String,

    /// Decay function: exponential, linear, gauss
    #[serde(default = "default_decay_function")]
    pub decay_function: String,

    /// Time scale (e.g., "7d", "30d", "1h")
    pub scale: String,

    /// Offset before decay starts
    #[serde(default)]
    pub offset: Option<String>,

    /// Decay rate (0.0 to 1.0)
    #[serde(default = "default_decay_rate")]
    pub decay_rate: f32,
}

fn default_decay_function() -> String {
    "exponential".to_string()
}

fn default_decay_rate() -> f32 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBoostConfig {
    /// Field to match
    pub field: String,

    /// Match current context value
    #[serde(default)]
    pub match_current: bool,

    /// Boost multiplier
    #[serde(default = "default_boost_multiplier")]
    pub boost: f32,
}

fn default_boost_multiplier() -> f32 {
    1.5
}

/// Configuration for automatically indexed system fields.
/// These fields enable ranking features without requiring migration later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemFieldsConfig {
    /// Automatically add _indexed_at timestamp to all documents (default: true)
    /// Required for recency/freshness scoring
    #[serde(default = "default_indexed_at_enabled")]
    pub indexed_at: bool,

    /// Enable per-document _boost field (default: false)
    /// Allows setting custom boost multipliers per document for popularity signals
    #[serde(default)]
    pub document_boost: bool,
}

impl Default for SystemFieldsConfig {
    fn default() -> Self {
        Self {
            indexed_at: true,
            document_boost: false,
        }
    }
}

fn default_indexed_at_enabled() -> bool {
    true
}

/// Configuration for hybrid search defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridConfig {
    /// Default merge strategy: "rrf" or "weighted" (default: "rrf")
    #[serde(default = "default_merge_strategy")]
    pub default_strategy: String,

    /// RRF k parameter - higher values reduce rank influence (default: 60)
    #[serde(default = "default_rrf_k")]
    pub rrf_k: usize,

    /// Default text weight for weighted merge (default: 0.5)
    #[serde(default = "default_text_weight")]
    pub text_weight: f32,

    /// Default vector weight for weighted merge (default: 0.5)
    #[serde(default = "default_vector_weight_hybrid")]
    pub vector_weight: f32,
}

impl Default for HybridConfig {
    fn default() -> Self {
        Self {
            default_strategy: "rrf".to_string(),
            rrf_k: 60,
            text_weight: 0.5,
            vector_weight: 0.5,
        }
    }
}

fn default_merge_strategy() -> String {
    "rrf".to_string()
}

fn default_rrf_k() -> usize {
    60
}

fn default_text_weight() -> f32 {
    0.5
}

fn default_vector_weight_hybrid() -> f32 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphBackendConfig {
    pub path: String,
    pub edges: Vec<EdgeTypeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeTypeConfig {
    pub edge_type: String,
    pub from_field: String,
    pub to_field: String,
}

/// Replication configuration for distributed deployments
///
/// Controls how many replicas of each shard are maintained and
/// how they are placed across the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    /// Number of copies of each shard (including primary)
    /// Default: 1 (no replication)
    #[serde(default = "default_replication_factor")]
    pub factor: usize,

    /// Minimum number of replicas that must acknowledge a write
    /// Default: 1 (synchronous write to at least one replica)
    #[serde(default = "default_min_replicas")]
    pub min_replicas_for_write: usize,

    /// Placement strategy for replicas
    /// - "zone-aware": Spread replicas across zones (default)
    /// - "rack-aware": Spread replicas across racks within a zone
    /// - "none": No placement constraints
    #[serde(default = "default_placement_strategy")]
    pub placement_strategy: String,
}

fn default_replication_factor() -> usize {
    1
}

fn default_min_replicas() -> usize {
    1
}

fn default_placement_strategy() -> String {
    "zone-aware".to_string()
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            factor: default_replication_factor(),
            min_replicas_for_write: default_min_replicas(),
            placement_strategy: default_placement_strategy(),
        }
    }
}

impl ReplicationConfig {
    /// Validate replication configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.factor == 0 {
            return Err("Replication factor must be at least 1".to_string());
        }
        if self.min_replicas_for_write > self.factor {
            return Err(format!(
                "min_replicas_for_write ({}) cannot exceed replication factor ({})",
                self.min_replicas_for_write, self.factor
            ));
        }
        let valid_strategies = ["zone-aware", "rack-aware", "none"];
        if !valid_strategies.contains(&self.placement_strategy.as_str()) {
            return Err(format!(
                "Invalid placement strategy '{}'. Valid values: {:?}",
                self.placement_strategy, valid_strategies
            ));
        }
        Ok(())
    }

    /// Check if replication is enabled (factor > 1)
    pub fn is_replicated(&self) -> bool {
        self.factor > 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_backend_config() {
        let yaml = r#"
collection: test_collection
backends:
  text:
    fields:
      - name: title
        type: text
        stored: true
        indexed: true
"#;
        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(schema.collection, "test_collection");
        assert!(schema.backends.text.is_some());

        let text = schema.backends.text.unwrap();
        assert_eq!(text.fields.len(), 1);
        assert_eq!(text.fields[0].name, "title");
        assert_eq!(text.fields[0].field_type, FieldType::Text);
    }

    #[test]
    fn test_parse_vector_backend_config() {
        let yaml = r#"
collection: embeddings
backends:
  vector:
    embedding_field: content_vector
    dimension: 384
    distance: cosine
    hnsw_m: 16
    hnsw_ef_construction: 200
    hnsw_ef_search: 100
embedding_generation:
  enabled: false
  model: all-MiniLM-L6-v2
  source_field: content
  target_field: content_vector
"#;
        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(schema.collection, "embeddings");
        assert!(schema.backends.vector.is_some());

        let vector = schema.backends.vector.unwrap();
        assert_eq!(vector.embedding_field, "content_vector");
        assert_eq!(vector.dimension, 384);
        assert_eq!(vector.distance, VectorDistance::Cosine);
        assert_eq!(vector.hnsw_m, 16);

        assert!(schema.embedding_generation.is_some());
        let emb_gen = schema.embedding_generation.unwrap();
        assert_eq!(emb_gen.enabled, false);
        assert_eq!(emb_gen.model, "all-MiniLM-L6-v2");
    }

    #[test]
    fn test_parse_facet_config() {
        let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
facets:
  allowed:
    - type
    - status
  default:
    - type
  configs:
    - field: type
      type: terms
      size: 10
"#;

        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
        let facets = schema.facets.unwrap();

        assert_eq!(facets.allowed.len(), 2);
        assert_eq!(facets.default.len(), 1);
        assert_eq!(facets.configs.len(), 1);
        assert_eq!(facets.configs[0].facet_type, FacetType::Terms);
    }

    #[test]
    fn test_parse_boosting_config() {
        let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
boosting:
  recency:
    field: timestamp
    decay_function: exponential
    scale: 7d
    decay_rate: 0.5
  context:
    - field: project_id
      match_current: true
      boost: 2.0
  field_weights:
    title: 2.0
    content: 1.0
"#;

        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
        let boosting = schema.boosting.unwrap();

        assert!(boosting.recency.is_some());
        assert_eq!(boosting.context.len(), 1);
        assert_eq!(boosting.field_weights.get("title"), Some(&2.0));
    }

    #[test]
    fn test_system_fields_default() {
        let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
"#;
        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();

        // _indexed_at should be enabled by default
        assert!(schema.system_fields.indexed_at);
        // _boost should be disabled by default
        assert!(!schema.system_fields.document_boost);
    }

    #[test]
    fn test_system_fields_custom() {
        let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
system_fields:
  indexed_at: false
  document_boost: true
"#;
        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();

        assert!(!schema.system_fields.indexed_at);
        assert!(schema.system_fields.document_boost);
    }

    #[test]
    fn test_hybrid_config() {
        let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
  vector:
    embedding_field: embedding
    dimension: 384
hybrid:
  default_strategy: weighted
  rrf_k: 100
  text_weight: 0.7
  vector_weight: 0.3
"#;
        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
        let hybrid = schema.hybrid.unwrap();

        assert_eq!(hybrid.default_strategy, "weighted");
        assert_eq!(hybrid.rrf_k, 100);
        assert_eq!(hybrid.text_weight, 0.7);
        assert_eq!(hybrid.vector_weight, 0.3);
    }

    #[test]
    fn test_bm25_config() {
        let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
    bm25_k1: 1.5
    bm25_b: 0.8
"#;
        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
        let text = schema.backends.text.unwrap();

        assert_eq!(text.bm25_k1, Some(1.5));
        assert_eq!(text.bm25_b, Some(0.8));
    }

    #[test]
    fn test_replication_config() {
        let yaml = r#"
collection: products
backends:
  text:
    fields:
      - name: title
        type: text
        indexed: true
  vector:
    embedding_field: embedding
    dimension: 384
replication:
  factor: 3
  min_replicas_for_write: 2
  placement_strategy: zone-aware
"#;
        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
        let replication = schema.replication.unwrap();

        assert_eq!(replication.factor, 3);
        assert_eq!(replication.min_replicas_for_write, 2);
        assert_eq!(replication.placement_strategy, "zone-aware");
        assert!(replication.is_replicated());
        assert!(replication.validate().is_ok());
    }

    #[test]
    fn test_replication_config_default() {
        let yaml = r#"
collection: test
backends:
  text:
    fields:
      - name: content
        type: text
        indexed: true
"#;
        let schema: CollectionSchema = serde_yaml::from_str(yaml).unwrap();
        assert!(schema.replication.is_none());

        let default = ReplicationConfig::default();
        assert_eq!(default.factor, 1);
        assert!(!default.is_replicated());
    }

    #[test]
    fn test_replication_config_validation() {
        let mut config = ReplicationConfig::default();

        // Factor 0 is invalid
        config.factor = 0;
        assert!(config.validate().is_err());

        // min_replicas > factor is invalid
        config.factor = 2;
        config.min_replicas_for_write = 3;
        assert!(config.validate().is_err());

        // Invalid placement strategy
        config.min_replicas_for_write = 1;
        config.placement_strategy = "invalid".to_string();
        assert!(config.validate().is_err());

        // Valid config
        config.placement_strategy = "zone-aware".to_string();
        assert!(config.validate().is_ok());
    }
}
