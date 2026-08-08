use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Distance metric for vector similarity
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
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
#[derive(Clone, Serialize, Deserialize)]
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

/// On-disk header for the v2 graph format.
/// Magic "PRH2" + bincode-serialized metadata + HnswMap (includes full graph structure).
const MAGIC_V2: &[u8; 4] = b"PRH2";
const MAGIC_V3: &[u8; 4] = b"PRH3";

/// Metadata header serialized before the HnswMap
#[cfg(feature = "vector-instant")]
#[derive(Serialize, Deserialize)]
struct GraphMeta {
    dimensions: usize,
    metric: Metric,
    m: usize,
    ef_construction: usize,
    rebuild_threshold: usize,
}

#[cfg(feature = "vector-instant")]
#[derive(Serialize, Deserialize)]
struct GraphMetaV3 {
    dimensions: usize,
    metric: Metric,
    m: usize,
    ef_construction: usize,
    rebuild_threshold: usize,
    unbuilt_keys: Vec<u32>,
    unbuilt_points: Vec<PointVec>,
}

#[cfg(feature = "vector-instant")]
pub struct InstantDistanceAdapter {
    dimensions: usize,
    metric: Metric,
    #[allow(dead_code)]
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

    pub fn extract_all_points(&self) -> std::collections::HashMap<u32, Vec<f32>> {
        let mut map = std::collections::HashMap::new();
        for (i, key) in self.keys.iter().enumerate() {
            map.insert(*key, self.points[i].v.clone());
        }
        map
    }

    /// Return the stored vector for a single key, if present.
    pub fn get_point(&self, key: u32) -> Option<Vec<f32>> {
        self.keys
            .iter()
            .position(|k| *k == key)
            .map(|i| self.points[i].v.clone())
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
        if self.hnsw.is_none() {
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
        let iter = hnsw.search(&query, &mut search);
        let mut out = Vec::new();
        for item in iter {
            out.push((*item.value, 1.0 - item.distance));
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
        // V3 format: "PRH3" magic + bincode(GraphMetaV3) + bincode(HnswMap)
        // This serializes the full HNSW graph structure and any unbuilt points.
        if let Some(ref hnsw) = self.hnsw {
            let unbuilt_keys = self.keys[self.built_size..].to_vec();
            let unbuilt_points = self.points[self.built_size..].to_vec();
            let meta = GraphMetaV3 {
                dimensions: self.dimensions,
                metric: self.metric,
                m: self.m,
                ef_construction: self.ef_construction,
                rebuild_threshold: self.rebuild_threshold,
                unbuilt_keys,
                unbuilt_points,
            };
            let meta_bytes = bincode::serialize(&meta).map_err(|e| {
                crate::error::Error::Backend(format!("bincode serialize meta v3: {}", e))
            })?;
            let hnsw_bytes = bincode::serialize(hnsw).map_err(|e| {
                crate::error::Error::Backend(format!("bincode serialize hnsw: {}", e))
            })?;
            let meta_len = meta_bytes.len() as u32;
            let mut buf = Vec::with_capacity(4 + 4 + meta_bytes.len() + hnsw_bytes.len());
            buf.extend_from_slice(MAGIC_V3);
            buf.extend_from_slice(&meta_len.to_le_bytes());
            buf.extend_from_slice(&meta_bytes);
            buf.extend_from_slice(&hnsw_bytes);
            std::fs::write(path, &buf)?;
            return Ok(());
        }

        // Fallback: save v1 binary (points only) if no graph built yet
        self.save_v1(path)
    }

    fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read(path)?;

        // V3 graph format (starts with "PRH3")
        if data.len() >= 4 && &data[0..4] == MAGIC_V3 {
            return Self::load_v3(&data[4..]);
        }

        // V2 graph format (starts with "PRH2")
        if data.len() >= 4 && &data[0..4] == MAGIC_V2 {
            return Self::load_v2(&data[4..]);
        }

        // V1 binary format (starts with "PRHW")
        if data.len() >= 22 && &data[0..4] == b"PRHW" {
            return Self::load_binary(&data);
        }

        // Legacy JSON format
        Self::load_json(&data)
    }

    fn len(&self) -> usize {
        self.keys.len()
    }
}

#[cfg(feature = "vector-instant")]
impl InstantDistanceAdapter {
    /// Load v2 format: bincode-serialized GraphMeta + HnswMap — no rebuild needed
    fn load_v2(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(crate::error::Error::Backend("V2 index too short".into()));
        }
        let meta_len = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        if data.len() < 4 + meta_len {
            return Err(crate::error::Error::Backend(
                "V2 index meta truncated".into(),
            ));
        }
        let meta: GraphMeta = bincode::deserialize(&data[4..4 + meta_len]).map_err(|e| {
            crate::error::Error::Backend(format!("bincode deserialize meta: {}", e))
        })?;
        let hnsw: HnswMap<PointVec, u32> =
            bincode::deserialize(&data[4 + meta_len..]).map_err(|e| {
                crate::error::Error::Backend(format!("bincode deserialize hnsw: {}", e))
            })?;

        // Reconstruct flat point/key arrays from the HnswMap for mutation support.
        // iter() yields (PointId, &P) in internal order; values[] is parallel to points.
        let mut points = Vec::new();
        let mut keys = Vec::new();
        for (_pid, point) in hnsw.iter() {
            points.push(point.clone());
        }
        keys.extend_from_slice(&hnsw.values);

        let built_size = keys.len();
        Ok(Self {
            dimensions: meta.dimensions,
            metric: meta.metric,
            m: meta.m,
            ef_construction: meta.ef_construction,
            keys,
            points,
            hnsw: Some(hnsw),
            searcher: Mutex::new(Search::default()),
            rebuild_threshold: meta.rebuild_threshold,
            built_size,
        })
    }

    /// Load v3 format: bincode-serialized GraphMetaV3 + HnswMap. Includes unbuilt points.
    fn load_v3(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(crate::error::Error::Backend("V3 index too short".into()));
        }
        let meta_len = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        if data.len() < 4 + meta_len {
            return Err(crate::error::Error::Backend(
                "V3 index meta truncated".into(),
            ));
        }
        let meta: GraphMetaV3 = bincode::deserialize(&data[4..4 + meta_len]).map_err(|e| {
            crate::error::Error::Backend(format!("bincode deserialize meta: {}", e))
        })?;
        let hnsw: HnswMap<PointVec, u32> =
            bincode::deserialize(&data[4 + meta_len..]).map_err(|e| {
                crate::error::Error::Backend(format!("bincode deserialize hnsw: {}", e))
            })?;

        let mut points = Vec::new();
        let mut keys = Vec::new();
        let values = hnsw.values.clone();
        for (i, p) in hnsw.iter().enumerate() {
            points.push(p.1.clone());
            keys.push(values[i]);
        }

        let built_size = keys.len();

        // Append unbuilt points from meta
        keys.extend(meta.unbuilt_keys);
        points.extend(meta.unbuilt_points);

        Ok(Self {
            dimensions: meta.dimensions,
            metric: meta.metric,
            m: meta.m,
            ef_construction: meta.ef_construction,
            keys,
            points,
            hnsw: Some(hnsw),
            searcher: Mutex::new(Search::default()),
            rebuild_threshold: meta.rebuild_threshold,
            built_size,
        })
    }

    /// Load legacy JSON format
    fn load_json(data: &[u8]) -> Result<Self> {
        let data: serde_json::Value = serde_json::from_slice(data)?;
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

    /// Save v1 binary format (points only, no graph — requires rebuild on load)
    fn save_v1(&self, path: &Path) -> Result<()> {
        let num_points = self.keys.len() as u32;
        let mut buf = Vec::with_capacity(
            22 + (num_points as usize * 4) + (num_points as usize * self.dimensions * 4),
        );
        buf.extend_from_slice(b"PRHW"); // magic
        buf.push(1u8); // version
        buf.push(match self.metric {
            Metric::Cosine => 0,
            Metric::Euclidean => 1,
            Metric::DotProduct => 2,
        });
        buf.extend_from_slice(&(self.dimensions as u32).to_le_bytes());
        buf.extend_from_slice(&num_points.to_le_bytes());
        buf.extend_from_slice(&(self.m as u16).to_le_bytes());
        buf.extend_from_slice(&(self.ef_construction as u16).to_le_bytes());
        buf.extend_from_slice(&(self.rebuild_threshold as u32).to_le_bytes());
        for &k in &self.keys {
            buf.extend_from_slice(&k.to_le_bytes());
        }
        for p in &self.points {
            for &f in &p.v {
                buf.extend_from_slice(&f.to_le_bytes());
            }
        }
        std::fs::write(path, &buf)?;
        Ok(())
    }

    fn load_binary(data: &[u8]) -> Result<Self> {
        if data.len() < 22 {
            return Err(crate::error::Error::Backend(
                "Binary index too short".into(),
            ));
        }
        let _version = data[4];
        let metric = match data[5] {
            0 => Metric::Cosine,
            1 => Metric::Euclidean,
            2 => Metric::DotProduct,
            _ => Metric::Cosine,
        };
        let dimensions = u32::from_le_bytes(data[6..10].try_into().unwrap()) as usize;
        let num_points = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;
        let m = u16::from_le_bytes(data[14..16].try_into().unwrap()) as usize;
        let ef_construction = u16::from_le_bytes(data[16..18].try_into().unwrap()) as usize;
        let rebuild_threshold = u32::from_le_bytes(data[18..22].try_into().unwrap()) as usize;

        let keys_start = 22;
        let keys_end = keys_start + num_points * 4;
        let points_end = keys_end + num_points * dimensions * 4;

        if data.len() < points_end {
            return Err(crate::error::Error::Backend(format!(
                "Binary index truncated: expected {} bytes, got {}",
                points_end,
                data.len()
            )));
        }

        // Read keys
        let mut keys = Vec::with_capacity(num_points);
        for i in 0..num_points {
            let off = keys_start + i * 4;
            keys.push(u32::from_le_bytes(data[off..off + 4].try_into().unwrap()));
        }

        // Read points — flat f32 arrays
        let mut points = Vec::with_capacity(num_points);
        let float_data = &data[keys_end..points_end];
        for i in 0..num_points {
            let off = i * dimensions * 4;
            let mut v = Vec::with_capacity(dimensions);
            for d in 0..dimensions {
                let fo = off + d * 4;
                v.push(f32::from_le_bytes(
                    float_data[fo..fo + 4].try_into().unwrap(),
                ));
            }
            points.push(PointVec { v, metric });
        }

        let mut adapter = Self {
            dimensions,
            metric,
            m,
            ef_construction,
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
}

// When vector-instant is enabled (including when both are enabled), prefer it.
// When only vector-usearch is enabled, it will be used once implemented.
#[cfg(feature = "vector-instant")]
pub type HnswBackend = InstantDistanceAdapter;

// TODO: Implement UsearchAdapter and use it when only vector-usearch is enabled
// #[cfg(all(feature = "vector-usearch", not(feature = "vector-instant")))]
// pub type HnswBackend = UsearchAdapter;

#[cfg(not(any(feature = "vector-instant", feature = "vector-usearch")))]
compile_error!("Must enable either vector-instant or vector-usearch feature");
