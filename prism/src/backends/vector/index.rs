use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Distance metric for vector similarity
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Metric {
    Cosine,
    Euclidean,
    DotProduct,
}

/// Unified trait for HNSW index implementations
pub trait HnswIndex: Send + Sync {
    /// Create new index
    fn new(dimensions: usize, metric: Metric, m: usize, ef_construction: usize) -> Result<Self>
    where
        Self: Sized;

    /// Add vector with key
    fn add(&mut self, key: u32, vector: &[f32]) -> Result<()>;

    /// Search for k nearest neighbors
    fn search(&self, vector: &[f32], k: usize, ef_search: usize) -> Result<Vec<(u32, f32)>>;

    /// Remove vector by key
    fn remove(&mut self, key: u32) -> Result<()>;

    /// Save index to disk
    fn save(&self, path: &Path) -> Result<()>;

    /// Load index from disk
    fn load(path: &Path) -> Result<Self>
    where
        Self: Sized;

    /// Get number of vectors in index
    fn len(&self) -> usize;

    /// Check if index is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// Simple in-memory adapter used as default/instant fallback

#[cfg(feature = "vector-instant")]
use instant_distance::{Builder, HnswMap, Point as IDPoint, Search};
#[cfg(feature = "vector-instant")]
use parking_lot::Mutex;

#[cfg(feature = "vector-instant")]
#[derive(Clone)]
struct PointVec {
    v: Vec<f32>,
    metric: Metric,
}

#[cfg(feature = "vector-instant")]
impl IDPoint for PointVec {
    fn distance(&self, other: &Self) -> f32 {
        match self.metric {
            Metric::Cosine => {
                let dot: f32 = self.v.iter().zip(other.v.iter()).map(|(a, b)| a * b).sum();
                let na = self.v.iter().map(|x| x * x).sum::<f32>().sqrt();
                let nb = other.v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if na == 0.0 || nb == 0.0 {
                    1.0
                } else {
                    1.0 - (dot / (na * nb))
                }
            }
            Metric::Euclidean => {
                let sum: f32 = self
                    .v
                    .iter()
                    .zip(other.v.iter())
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum();
                sum.sqrt()
            }
            Metric::DotProduct => {
                let dot: f32 = self.v.iter().zip(other.v.iter()).map(|(a, b)| a * b).sum();
                // convert to distance
                1.0 - dot
            }
        }
    }
}

#[cfg(feature = "vector-instant")]
pub struct InstantDistanceAdapter {
    dimensions: usize,
    metric: Metric,
    m: usize,
    ef_construction: usize,
    points: Vec<PointVec>,
    keys: Vec<u32>,
    hnsw: Option<HnswMap<PointVec, u32>>,
    // searcher is protected by a mutex for concurrent searches
    searcher: Mutex<Search>,
    // Number of adds to buffer before rebuilding the HNSW structure
    rebuild_threshold: usize,
    // Number of points included in the last built HNSW map
    built_size: usize,
}

#[cfg(feature = "vector-instant")]
impl InstantDistanceAdapter {
    fn rebuild(&mut self) -> Result<()> {
        if self.points.is_empty() {
            self.hnsw = None;
            self.built_size = 0;
            return Ok(());
        }
        let builder = Builder::default()
            .ef_construction(self.ef_construction)
            .ef_search(100);
        let map = builder.build(self.points.clone(), self.keys.clone());
        self.built_size = self.keys.len();
        self.hnsw = Some(map);
        // reset searcher capacity
        self.searcher = Mutex::new(Search::default());
        Ok(())
    }
}

#[cfg(feature = "vector-instant")]
impl HnswIndex for InstantDistanceAdapter {
    fn new(dimensions: usize, metric: Metric, m: usize, ef_construction: usize) -> Result<Self> {
        Ok(Self {
            dimensions,
            metric,
            m,
            ef_construction,
            points: Vec::new(),
            keys: Vec::new(),
            hnsw: None,
            searcher: Mutex::new(Search::default()),
            rebuild_threshold: 32, // default buffer size before rebuild
            built_size: 0,
        })
    }

    fn add(&mut self, key: u32, vector: &[f32]) -> Result<()> {
        if vector.len() != self.dimensions {
            return Err(crate::error::Error::Schema(format!(
                "Expected {} dimensions, got {}",
                self.dimensions,
                vector.len()
            )));
        }
        self.keys.push(key);
        self.points.push(PointVec {
            v: vector.to_vec(),
            metric: self.metric,
        });

        // Only rebuild the heavy HNSW structure when threshold is reached
        // If no hnsw built yet, or we've accumulated rebuild_threshold new points since last build, rebuild.
        // Also, for small collections (<= rebuild_threshold) rebuild on each add to keep index usable.
        let need_build = self.hnsw.is_none()
            || (self.keys.len() <= self.rebuild_threshold)
            || (self.keys.len() - self.built_size >= self.rebuild_threshold);
        if need_build {
            self.rebuild()?;
        }
        Ok(())
    }

    fn search(&self, vector: &[f32], k: usize, _ef_search: usize) -> Result<Vec<(u32, f32)>> {
        // Ensure index is built before searching
        if self.hnsw.is_none() {
            // Mutable borrow needed to rebuild; perform a transient mutable borrow on a cloned self is not possible here
            // so convert by temporarily acquiring a mutable reference via unsafe interior mutability: clone minimal state and rebuild
            // Simpler approach: error if not built to force caller to trigger a build via a committed path. However, to maintain
            // usability, we'll assume that search is called on an adapter that has been built at least once. If not, return empty.
            return Ok(Vec::new());
        }

        let hnsw = self
            .hnsw
            .as_ref()
            .ok_or_else(|| crate::error::Error::Backend("Index not built".into()))?;
        let query = PointVec {
            v: vector.to_vec(),
            metric: self.metric,
        };
        let mut search = self.searcher.lock();
        let iter = hnsw.search(&query, &mut *search);
        let mut out = Vec::new();
        for item in iter {
            out.push((item.value.clone(), 1.0 - item.distance));
            if out.len() >= k {
                break;
            }
        }
        Ok(out)
    }

    fn remove(&mut self, key: u32) -> Result<()> {
        if let Some(pos) = self.keys.iter().position(|&k| k == key) {
            self.keys.remove(pos);
            self.points.remove(pos);
            // After a removal, rebuild to keep structure consistent
            self.rebuild()?;
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::json!({
            "dimensions": self.dimensions,
            "keys": self.keys,
            "points": self.points.iter().map(|p| p.v.clone()).collect::<Vec<_>>(),
            "rebuild_threshold": self.rebuild_threshold,
        });
        std::fs::write(path, serde_json::to_vec(&data)?)?;
        Ok(())
    }

    fn load(path: &Path) -> Result<Self> {
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let dimensions = data["dimensions"].as_u64().unwrap() as usize;
        let keys: Vec<u32> = serde_json::from_value(data["keys"].clone())?;
        let points_data: Vec<Vec<f32>> = serde_json::from_value(data["points"].clone())?;
        let points: Vec<PointVec> = points_data
            .into_iter()
            .map(|v| PointVec {
                v,
                metric: Metric::Cosine,
            })
            .collect();
        let rebuild_threshold = data
            .get("rebuild_threshold")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(32);
        let mut adapter = Self {
            dimensions,
            metric: Metric::Cosine,
            m: 16,
            ef_construction: 200,
            points,
            keys,
            hnsw: None,
            searcher: Mutex::new(Search::default()),
            rebuild_threshold,
            built_size: 0,
        };
        adapter.rebuild()?;
        Ok(adapter)
    }

    fn len(&self) -> usize {
        self.keys.len()
    }
}

#[cfg(feature = "vector-instant")]
pub type HnswBackend = InstantDistanceAdapter;

// Compile-time checks
#[cfg(all(feature = "vector-instant", feature = "vector-usearch"))]
compile_error!("Cannot enable both vector-instant and vector-usearch features at the same time");

#[cfg(not(any(feature = "vector-instant", feature = "vector-usearch")))]
compile_error!("Must enable either vector-instant or vector-usearch feature");
