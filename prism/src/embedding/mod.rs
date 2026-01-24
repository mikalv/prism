//! Embedding generation using ONNX models

#[cfg(feature = "embedding-gen")]
mod inference;
#[cfg(feature = "embedding-gen")]
mod model;

#[cfg(feature = "embedding-gen")]
pub use inference::Embedder;
#[cfg(feature = "embedding-gen")]
pub use model::{ModelCache, ModelConfig};

#[cfg(not(feature = "embedding-gen"))]
pub struct Embedder;

#[cfg(not(feature = "embedding-gen"))]
impl Embedder {
    pub fn new<T>(_config: T) -> anyhow::Result<Self> {
        anyhow::bail!("Embedding generation not enabled. Compile with --features embedding-gen")
    }
}
