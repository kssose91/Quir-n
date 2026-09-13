//! Sled-based storage layer (pure Rust, no C++ dependencies).

use crate::error::{BrainError, Result};
use crate::storage::cf::all_tree_names;
use sled::{Db, Tree};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

/// Storage wrapper around Sled embedded database.
#[derive(Clone)]
pub struct Storage {
    db: Arc<Db>,
    trees: Arc<HashMap<String, Tree>>,
    // Serializes ledger writers and verification across cloned database handles.
    ledger_lock: Arc<Mutex<()>>,
}

impl Storage {
    /// Open or create a Sled database at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Open database
        let db = sled::open(path)
            .map_err(|e| BrainError::Internal(anyhow::anyhow!("Failed to open sled: {}", e)))?;

        // Open all named trees (equivalent to column families)
        let mut trees = HashMap::new();
        for name in all_tree_names() {
            let tree = db.open_tree(name).map_err(|e| {
                BrainError::Internal(anyhow::anyhow!("Failed to open tree {}: {}", name, e))
            })?;
            trees.insert(name.to_string(), tree);
        }

        tracing::info!("Opened Sled database at {:?}", path);

        Ok(Self {
            db: Arc::new(db),
            trees: Arc::new(trees),
            ledger_lock: Arc::new(Mutex::new(())),
        })
    }

    pub(crate) fn lock_ledger(&self) -> Result<MutexGuard<'_, ()>> {
        self.ledger_lock.lock().map_err(|_| {
            BrainError::Internal(anyhow::anyhow!("Ledger lock poisoned; reopen and verify the database"))
        })
    }

    /// Get a tree (column family equivalent) by name.
    pub fn tree(&self, name: &str) -> Option<&Tree> {
        self.trees.get(name)
    }

    /// Get a tree, returning an error if not found.
    fn tree_required(&self, name: &str) -> Result<&Tree> {
        self.tree(name)
            .ok_or_else(|| BrainError::Internal(anyhow::anyhow!("Tree not found: {}", name)))
    }

    /// Put a key-value pair in a tree.
    pub fn put(&self, tree_name: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let tree = self.tree_required(tree_name)?;
        tree.insert(key, value)
            .map_err(|e| BrainError::Internal(anyhow::anyhow!("Insert failed: {}", e)))?;
        Ok(())
    }

    /// Get a value by key from a tree.
    pub fn get(&self, tree_name: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let tree = self.tree_required(tree_name)?;
        match tree.get(key) {
            Ok(Some(ivec)) => Ok(Some(ivec.to_vec())),
            Ok(None) => Ok(None),
            Err(e) => Err(BrainError::Internal(anyhow::anyhow!("Get failed: {}", e))),
        }
    }

    /// Delete a key from a tree.
    pub fn delete(&self, tree_name: &str, key: &[u8]) -> Result<()> {
        let tree = self.tree_required(tree_name)?;
        tree.remove(key)
            .map_err(|e| BrainError::Internal(anyhow::anyhow!("Delete failed: {}", e)))?;
        Ok(())
    }

    /// Delete all keys from a tree.
    pub fn clear_tree(&self, tree_name: &str) -> Result<u64> {
        let tree = self.tree_required(tree_name)?;
        let keys: Vec<_> = tree.iter().keys().filter_map(|item| item.ok()).collect();
        let removed = keys.len() as u64;

        for key in keys {
            tree.remove(key)
                .map_err(|e| BrainError::Internal(anyhow::anyhow!("Clear tree failed: {}", e)))?;
        }

        Ok(removed)
    }

    /// Flush all data to disk.
    pub fn flush(&self) -> Result<()> {
        self.db
            .flush()
            .map_err(|e| BrainError::Internal(anyhow::anyhow!("Flush failed: {}", e)))?;
        Ok(())
    }

    /// Iterate over a tree with a prefix.
    pub fn prefix_iter<'a>(
        &'a self,
        tree_name: &str,
        prefix: &[u8],
    ) -> Result<impl Iterator<Item = (Vec<u8>, Vec<u8>)> + 'a> {
        let tree = self.tree_required(tree_name)?;
        let prefix_vec = prefix.to_vec();
        Ok(tree
            .scan_prefix(&prefix_vec)
            .filter_map(|r| r.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))))
    }

    /// Iterate over all keys in a tree.
    pub fn iter_tree<'a>(
        &'a self,
        tree_name: &str,
    ) -> Result<impl Iterator<Item = (Vec<u8>, Vec<u8>)> + 'a> {
        let tree = self.tree_required(tree_name)?;
        Ok(tree
            .iter()
            .filter_map(|r| r.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))))
    }

    /// Count keys in a tree.
    pub fn count(&self, tree_name: &str) -> Result<u64> {
        let tree = self.tree_required(tree_name)?;
        Ok(tree.len() as u64)
    }

    /// Batch insert multiple key-value pairs atomically.
    pub fn batch_insert(&self, tree_name: &str, pairs: Vec<(Vec<u8>, Vec<u8>)>) -> Result<()> {
        let tree = self.tree_required(tree_name)?;
        let mut batch = sled::Batch::default();
        for (key, value) in pairs {
            batch.insert(key, value);
        }
        tree.apply_batch(batch)
            .map_err(|e| BrainError::Internal(anyhow::anyhow!("Batch insert failed: {}", e)))?;
        Ok(())
    }

    /// Iterate over all keys in a tree in REVERSE order (newest first for time keys).
    pub fn iter_tree_rev<'a>(
        &'a self,
        tree_name: &str,
    ) -> Result<impl Iterator<Item = (Vec<u8>, Vec<u8>)> + 'a> {
        let tree = self.tree_required(tree_name)?;
        Ok(tree
            .iter()
            .rev()
            .filter_map(|r| r.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))))
    }

    /// Iterate over keys in a tree within a key range [from..to] (inclusive start, exclusive end).
    pub fn range_iter<'a>(
        &'a self,
        tree_name: &str,
        from: &[u8],
        to: &[u8],
    ) -> Result<impl Iterator<Item = (Vec<u8>, Vec<u8>)> + 'a> {
        let tree = self.tree_required(tree_name)?;
        let from_vec = from.to_vec();
        let to_vec = to.to_vec();
        Ok(tree
            .range(from_vec..to_vec)
            .filter_map(|r| r.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_open_and_write() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();

        // Write and read
        storage.put("kv", b"key1", b"value1").unwrap();
        let val = storage.get("kv", b"key1").unwrap();
        assert_eq!(val, Some(b"value1".to_vec()));

        // Delete
        storage.delete("kv", b"key1").unwrap();
        let val = storage.get("kv", b"key1").unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn test_prefix_scan() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();

        storage.put("kv", b"prefix:a", b"1").unwrap();
        storage.put("kv", b"prefix:b", b"2").unwrap();
        storage.put("kv", b"other:c", b"3").unwrap();

        let results: Vec<_> = storage.prefix_iter("kv", b"prefix:").unwrap().collect();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_clear_tree() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();

        storage.put("kv", b"a", b"1").unwrap();
        storage.put("kv", b"b", b"2").unwrap();

        let removed = storage.clear_tree("kv").unwrap();
        assert_eq!(removed, 2);
        assert_eq!(storage.count("kv").unwrap(), 0);
    }
}
