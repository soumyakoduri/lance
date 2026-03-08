// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: Copyright The Lance Authors

//! RGW (RADOS Gateway) Object Store Provider
//!
//! This module provides integration with Ceph RADOS Gateway through the RGW SAL C API.
//! It allows LanceDB to use RGW as an object store backend by wrapping the RGWObjectStore
//! implemented in lance-io.
//!
//! # Architecture
//!
//! Unlike other providers (S3, GCS, Azure) which create fresh object store instances,
//! the RGW provider holds pre-initialized driver and DPP pointers that are passed from
//! external C/C++ code. These pointers are used to create RGWObjectStore instances.
//!
//! The RGWObjectStore is implemented entirely within lance-io using FFI bindings to
//! the RGW SAL C API (from Ceph). No changes to arrow-rs-object-store are required.
//!
//! # Usage
//!
//! ```ignore
//! use std::sync::Arc;
//! use std::ffi::c_void;
//! use lance_io::object_store::{ObjectStoreParams, ObjectStoreRegistry};
//! use lance_io::object_store::providers::rgw::RGWStoreProvider;
//!
//! // Create provider with RGW driver and DPP pointers (from external C code)
//! let provider = unsafe {
//!     RGWStoreProvider::new(driver_ptr, dpp_ptr)
//! };
//!
//! // Register the provider
//! let mut registry = ObjectStoreRegistry::default();
//! registry.register("rgw", Arc::new(provider));
//!
//! // Use with LanceDB via URL
//! let url = Url::parse("rgw://my-bucket/path/to/data").unwrap();
//! let store = registry.get_store(&url, &params).await?;
//! ```

use std::{collections::HashMap, ffi::c_void, sync::Arc};

use object_store::{path::Path, ObjectStore as OSObjectStore};
use url::Url;

use crate::object_store::{
    rgw::RGWObjectStore, ObjectStore, ObjectStoreParams, ObjectStoreProvider, StorageOptions,
    DEFAULT_CLOUD_IO_PARALLELISM, DEFAULT_LOCAL_BLOCK_SIZE, DEFAULT_MAX_IOP_SIZE,
};
use lance_core::error::{Error, Result};

/// RGW Object Store Provider
///
/// This provider wraps pre-initialized RGW driver and DPP pointers and creates
/// RGWObjectStore instances for specific buckets.
///
/// # Safety
///
/// The provider holds raw C pointers (driver and dpp) that must remain valid for
/// the lifetime of the provider and any stores it creates.
///
/// # Thread Safety
///
/// The provider is marked as Send+Sync based on the assumption that the underlying
/// RGW driver is thread-safe. The caller must ensure this when creating the provider.
#[derive(Debug, Clone)]
pub struct RGWStoreProvider {
    /// Pointer to the RGW Driver (from RGW SAL C API)
    driver: *mut c_void,
    /// Pointer to the DoutPrefixProvider for logging (can be null)
    dpp: *const c_void,
}

impl RGWStoreProvider {
    /// Create a new RGW store provider with driver and DPP pointers
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `driver` points to a valid, initialized RGW Driver instance
    /// - `dpp` points to a valid DoutPrefixProvider (or is null)
    /// - Both pointers remain valid for the lifetime of this provider and any stores it creates
    /// - The driver is properly initialized and connected to RADOS
    /// - The driver supports thread-safe operations (required for Send+Sync)
    ///
    /// # Arguments
    ///
    /// * `driver` - Pointer to initialized RGW Driver (from librgw)
    /// * `dpp` - Pointer to DoutPrefixProvider for logging (can be null)
    ///
    /// # Example
    ///
    /// ```ignore
    /// // In C/C++ code:
    /// // rgw::sal::Driver* driver = create_rados_driver(...);
    /// // DoutPrefixProvider* dpp = ...;
    ///
    /// // In Rust:
    /// let provider = unsafe {
    ///     RGWStoreProvider::new(driver as *mut c_void, dpp as *const c_void)
    /// };
    /// ```
    pub unsafe fn new(driver: *mut c_void, dpp: *const c_void) -> Self {
        Self { driver, dpp }
    }

    /// Get the driver pointer (for advanced use cases)
    pub fn driver(&self) -> *mut c_void {
        self.driver
    }

    /// Get the DPP pointer (for advanced use cases)
    pub fn dpp(&self) -> *const c_void {
        self.dpp
    }
}

// Safety: The user guarantees that driver and dpp are thread-safe when creating the provider
unsafe impl Send for RGWStoreProvider {}
unsafe impl Sync for RGWStoreProvider {}

#[async_trait::async_trait]
impl ObjectStoreProvider for RGWStoreProvider {
    async fn new_store(&self, base_path: Url, params: &ObjectStoreParams) -> Result<ObjectStore> {
        // Extract bucket name from URL (e.g., rgw://bucket-name/path)
        let bucket_name = base_path
            .host_str()
            .ok_or_else(|| {
                Error::invalid_input(format!("RGW URL must contain bucket name: {}", base_path))
            })?
            .to_string();

        // Build RGW ObjectStore using lance-io's RGW implementation
        // This uses FFI bindings to the RGW SAL C API (from Ceph)
        let rgw_store = unsafe { RGWObjectStore::new(bucket_name.clone(), self.driver, self.dpp) };

        let mut inner: Arc<dyn OSObjectStore> = Arc::new(rgw_store);

        // Apply any object store wrappers from params
        if let Some(wrapper) = &params.object_store_wrapper {
            let store_prefix = self.calculate_object_store_prefix(&base_path, params.storage_options())?;
            inner = wrapper.wrap(&store_prefix, inner);
        }

        // Configure ObjectStore with appropriate settings
        let block_size = params.block_size.unwrap_or(DEFAULT_LOCAL_BLOCK_SIZE);
        let storage_options = StorageOptions(params.storage_options().cloned().unwrap_or_default());
        let download_retry_count = storage_options.download_retry_count();

        Ok(ObjectStore {
            inner,
            scheme: "rgw".to_string(),
            block_size,
            max_iop_size: *DEFAULT_MAX_IOP_SIZE,
            use_constant_size_upload_parts: params.use_constant_size_upload_parts,
            // RGW lists are lexically ordered (like object stores, not local filesystems)
            list_is_lexically_ordered: params.list_is_lexically_ordered.unwrap_or(true),
            io_parallelism: DEFAULT_CLOUD_IO_PARALLELISM,
            download_retry_count,
            io_tracker: Default::default(),
            store_prefix: self.calculate_object_store_prefix(&base_path, params.storage_options())?,
        })
    }

    fn extract_path(&self, url: &Url) -> Result<Path> {
        // For rgw://bucket/path/to/file, extract "path/to/file"
        // The path is relative to the bucket root
        let path_str = url.path().trim_start_matches('/');
        Path::parse(path_str).map_err(|e| {
            Error::invalid_input(format!("Invalid path in RGW URL '{}': {}", url, e))
        })
    }

    fn calculate_object_store_prefix(
        &self,
        url: &Url,
        _storage_options: Option<&HashMap<String, String>>,
    ) -> Result<String> {
        // For RGW, the bucket name uniquely identifies the store
        // Format: rgw$bucket_name
        let bucket = url.host_str().ok_or_else(|| {
            Error::invalid_input(format!("RGW URL must contain bucket name: {}", url))
        })?;

        Ok(format!("rgw${}", bucket))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn test_provider_creation() {
        // Create provider with dummy pointers for testing
        let provider = unsafe { RGWStoreProvider::new(1 as *mut c_void, ptr::null()) };

        assert_eq!(provider.driver() as usize, 1);
        assert_eq!(provider.dpp() as usize, 0);
    }

    #[test]
    fn test_extract_path() {
        let provider = unsafe { RGWStoreProvider::new(1 as *mut c_void, ptr::null()) };

        let url = Url::parse("rgw://my-bucket/path/to/file.txt").unwrap();
        let path = provider.extract_path(&url).unwrap();

        assert_eq!(path, Path::from("path/to/file.txt"));
    }

    #[test]
    fn test_extract_path_root() {
        let provider = unsafe { RGWStoreProvider::new(1 as *mut c_void, ptr::null()) };

        let url = Url::parse("rgw://my-bucket/").unwrap();
        let path = provider.extract_path(&url).unwrap();

        assert_eq!(path, Path::from(""));
    }

    #[test]
    fn test_calculate_prefix() {
        let provider = unsafe { RGWStoreProvider::new(1 as *mut c_void, ptr::null()) };

        let url = Url::parse("rgw://my-bucket/path").unwrap();
        let prefix = provider
            .calculate_object_store_prefix(&url, None)
            .unwrap();

        assert_eq!(prefix, "rgw$my-bucket");
    }

    #[test]
    fn test_url_without_bucket() {
        let provider = unsafe { RGWStoreProvider::new(1 as *mut c_void, ptr::null()) };

        // URL without bucket should fail
        let url = Url::parse("rgw:///path").unwrap();
        assert!(provider.extract_path(&url).is_ok()); // Path extraction works
        assert!(provider.calculate_object_store_prefix(&url, None).is_err()); // But prefix fails
    }
}
