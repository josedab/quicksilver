//! Inline cache for property access optimization
//!
//! This module implements a polymorphic inline cache (PIC) for fast property access.

use rustc_hash::FxHashMap as HashMap;
use super::super::value::{Object, Value};

/// Inline cache size for property access
pub const IC_SIZE: usize = 256;

/// Maximum number of shapes per polymorphic IC slot
/// Increased from 4 to 12 for better polymorphic performance
pub const PIC_MAX_SHAPES: usize = 12;

/// A single shape-to-offset mapping in the polymorphic cache
#[derive(Clone, Default)]
pub struct ShapeEntry {
    /// Shape ID of the object (hash of property keys order)
    pub shape_id: u64,
    /// Offset where property was found
    pub offset: usize,
    /// Hit count for this shape
    pub hits: u32,
}

/// Entry in the polymorphic inline cache for property access
#[derive(Clone)]
pub struct InlineCacheEntry {
    /// Hash of the property name being cached
    pub name_hash: u64,
    /// Multiple shape entries for polymorphic dispatch
    pub shapes: [ShapeEntry; PIC_MAX_SHAPES],
    /// Number of active shape entries
    pub shape_count: u8,
    /// Whether this cache site is megamorphic (too many shapes)
    pub is_megamorphic: bool,
    /// Total hit count
    pub total_hits: u32,
}

impl Default for InlineCacheEntry {
    fn default() -> Self {
        Self {
            name_hash: 0,
            shapes: Default::default(),
            shape_count: 0,
            is_megamorphic: false,
            total_hits: 0,
        }
    }
}

impl InlineCacheEntry {
    /// Look up property offset for a given shape
    #[inline]
    pub fn lookup(&self, shape_id: u64) -> Option<usize> {
        if self.is_megamorphic {
            return None;
        }
        for i in 0..(self.shape_count as usize) {
            if self.shapes[i].shape_id == shape_id {
                return Some(self.shapes[i].offset);
            }
        }
        None
    }

    /// Add or update a shape entry
    #[inline]
    pub fn update(&mut self, shape_id: u64, offset: usize) {
        // Check if shape already exists
        for i in 0..(self.shape_count as usize) {
            if self.shapes[i].shape_id == shape_id {
                self.shapes[i].hits = self.shapes[i].hits.saturating_add(1);
                return;
            }
        }

        // Add new shape if we have room
        if (self.shape_count as usize) < PIC_MAX_SHAPES {
            let idx = self.shape_count as usize;
            self.shapes[idx] = ShapeEntry {
                shape_id,
                offset,
                hits: 1,
            };
            self.shape_count += 1;
        } else {
            // Too many shapes - mark as megamorphic
            self.is_megamorphic = true;
        }
        self.total_hits = self.total_hits.saturating_add(1);
    }
}

/// Simple hash function for property names
#[inline]
pub fn hash_property_name(name: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in name.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

/// Compute a shape ID for an object based on its property keys
/// This allows polymorphic IC to distinguish objects with different structures
#[inline]
pub fn compute_shape_id_raw(properties: &HashMap<String, Value>) -> u64 {
    // Simple shape ID based on number of properties and hash of keys
    let mut hash: u64 = properties.len() as u64;
    for key in properties.keys() {
        hash = hash.wrapping_mul(31).wrapping_add(hash_property_name(key));
    }
    hash
}

/// Get or compute the shape ID for an object, using cached value if available
/// This is O(1) for cache hits, O(properties) for cache misses
#[inline]
#[allow(dead_code)]
pub fn get_or_compute_shape_id(obj: &mut Object) -> u64 {
    if let Some(id) = obj.cached_shape_id {
        return id;
    }
    let id = compute_shape_id_raw(&obj.properties);
    obj.cached_shape_id = Some(id);
    id
}
