// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: Copyright The Lance Authors

//! RGW (RADOS Gateway) Object Store implementation for Lance
//!
//! This module provides an ObjectStore implementation backed by Ceph's RADOS Gateway (RGW)
//! using the RGW SAL C API. It integrates with Lance via the WrappingObjectStore pattern.
//!
//! # Architecture
//!
//! RGWObjectStore implements both:
//! - `object_store::ObjectStore` - for actual I/O operations
//! - `WrappingObjectStore` - for Lance integration via store_params
//!
//! # Usage Pattern (from lancedb-c)
//!
//! ```ignore
//! // 1. Create RGW store with driver/dpp (from C code)
//! let rgw_store = unsafe {
//!     RGWObjectStore::new("my-bucket", driver_ptr, dpp_ptr)
//! };
//!
//! // 2. Wrap in Arc for use as WrappingObjectStore
//! let wrapper = Arc::new(rgw_store) as Arc<dyn WrappingObjectStore>;
//!
//! // 3. Pass via ObjectStoreParams in table operations
//! let store_params = ObjectStoreParams {
//!     object_store_wrapper: Some(wrapper),
//!     ..Default::default()
//! };
//!
//! // 4. Use in write/read operations
//! db.create_table("name", data)
//!     .write_options(WriteOptions {
//!         lance_write_params: Some(WriteParams {
//!             store_params: Some(store_params),
//!             ..Default::default()
//!         }),
//!     })
//!     .execute()
//!     .await?;
//! ```

use std::ffi::CString;
use std::fmt;
use std::os::raw::{c_char, c_void};
use std::ptr;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{Utc, TimeZone};
use futures::stream::{self, BoxStream, StreamExt};
use object_store::{
    path::Path, GetOptions, GetRange, GetResult, GetResultPayload,
    ListResult, MultipartUpload, ObjectMeta, ObjectStore as OSObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, Result as OSResult,
};

use crate::object_store::WrappingObjectStore;

pub mod error;
mod ffi;
pub mod ffi_wrapper;
mod memory;

pub use error::RgwError;

use error::check_errno;
use memory::{RgwBuffer, RgwListResultWrapper, RgwString};

const STORE: &str = "RGW";

/// RGW ObjectStore implementation
///
/// Wraps a pre-initialized RGW driver and provides both ObjectStore and
/// WrappingObjectStore trait implementations for Lance integration.
///
/// # Thread Safety
///
/// Marked as Send+Sync based on the assumption that the underlying RGW driver is thread-safe.
/// The user must ensure this when constructing the ObjectStore.
#[derive(Clone)]
pub struct RGWObjectStore {
    bucket_name: String,
    driver: *mut c_void,
    dpp: *const c_void,
}

// Safety: The user guarantees that driver and dpp are thread-safe
unsafe impl Send for RGWObjectStore {}
unsafe impl Sync for RGWObjectStore {}

impl RGWObjectStore {
    /// Create a new RGW ObjectStore
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `driver` points to a valid, initialized RGW Driver instance
    /// - `dpp` points to a valid DoutPrefixProvider (or is null)
    /// - Both pointers remain valid for the lifetime of this ObjectStore
    /// - The driver is properly initialized and connected to RADOS
    /// - The driver supports thread-safe operations
    ///
    /// # Arguments
    ///
    /// * `bucket_name` - The RGW bucket name
    /// * `driver` - Pointer to initialized RGW Driver (from librgw)
    /// * `dpp` - Pointer to DoutPrefixProvider for logging (can be null)
    pub unsafe fn new(
        bucket_name: impl Into<String>,
        driver: *mut c_void,
        dpp: *const c_void,
    ) -> Self {
        Self {
            bucket_name: bucket_name.into(),
            driver,
            dpp,
        }
    }

    /// Get the bucket name
    pub fn bucket_name(&self) -> &str {
        &self.bucket_name
    }
}

impl fmt::Display for RGWObjectStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RGWObjectStore({})", self.bucket_name)
    }
}

impl fmt::Debug for RGWObjectStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RGWObjectStore")
            .field("bucket_name", &self.bucket_name)
            .field("driver", &self.driver)
            .field("dpp", &self.dpp)
            .finish()
    }
}

/// Implement WrappingObjectStore for Lance integration
///
/// The wrap() method returns self, replacing the placeholder ObjectStore
/// created from "rgw://" URL with the actual RGW store (with driver/dpp).
impl WrappingObjectStore for RGWObjectStore {
    fn wrap(
        &self,
        _store_prefix: &str,
        _original: Arc<dyn OSObjectStore>,
    ) -> Arc<dyn OSObjectStore> {
        // Return self as the ObjectStore, replacing the placeholder
        Arc::new(self.clone())
    }
}

#[async_trait]
impl OSObjectStore for RGWObjectStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        _opts: PutOptions,
    ) -> OSResult<PutResult> {
        // Collect payload bytes
        let data: Vec<u8> = payload.iter().flat_map(|b| b.iter()).copied().collect();

        // Clone values for the blocking task
        let bucket = self.bucket_name.clone();
        let key = location.to_string();
        let driver_ptr = self.driver as usize;
        let dpp_ptr = self.dpp as usize;
        let location_owned = location.clone();

        // Spawn blocking task for C API call
        let result = tokio::task::spawn_blocking(move || {
            // Cast pointers back
            let driver_ptr = driver_ptr as *mut c_void;
            let dpp_ptr = dpp_ptr as *const c_void;

            // Convert strings to C strings
            let bucket_c = CString::new(bucket).map_err(|e| object_store::Error::Generic {
                store: STORE,
                source: Box::new(e),
            })?;
            let key_c = CString::new(key).map_err(|e| object_store::Error::Generic {
                store: STORE,
                source: Box::new(e),
            })?;

            let mut etag_ptr: *mut c_char = ptr::null_mut();

            // Call RGW PUT API
            let ret = unsafe {
                ffi::rgw_put_object(
                    driver_ptr,
                    dpp_ptr,
                    bucket_c.as_ptr(),
                    key_c.as_ptr(),
                    data.as_ptr() as *const c_char,
                    data.len() as u64,
                    ptr::null_mut(), // obj_attributes
                    ptr::null(),     // conditionals
                    &mut etag_ptr,
                )
            };

            // Check for errors
            check_errno(ret, &location_owned)?;

            // Extract etag if present
            let etag = if !etag_ptr.is_null() {
                let etag_wrapper = unsafe { RgwString::from_raw(etag_ptr) };
                etag_wrapper.to_string()
            } else {
                None
            };

            Ok::<PutResult, object_store::Error>(PutResult {
                e_tag: etag,
                version: None,
            })
        })
        .await
        .map_err(|e| object_store::Error::Generic {
            store: STORE,
            source: Box::new(e),
        })??;

        Ok(result)
    }

    async fn put_multipart_opts(
        &self,
        _location: &Path,
        _opts: PutMultipartOptions,
    ) -> OSResult<Box<dyn MultipartUpload>> {
        Err(object_store::Error::NotImplemented)
    }

    async fn get_opts(&self, location: &Path, options: GetOptions) -> OSResult<GetResult> {
        let bucket = self.bucket_name.clone();
        let key = location.to_string();
        let driver_ptr = self.driver as usize;
        let dpp_ptr = self.dpp as usize;
        let location_clone = location.clone();

        let range = options.range.clone();

        // Spawn blocking task for C API call
        let (bytes, meta) = tokio::task::spawn_blocking(move || {
            // Cast pointers back
            let driver_ptr = driver_ptr as *mut c_void;
            let dpp_ptr = dpp_ptr as *const c_void;

            // Convert strings to C strings
            let bucket_c = CString::new(bucket).map_err(|e| object_store::Error::Generic {
                store: STORE,
                source: Box::new(e),
            })?;
            let key_c = CString::new(key).map_err(|e| object_store::Error::Generic {
                store: STORE,
                source: Box::new(e),
            })?;

            let mut buffer_ptr: *mut c_char = ptr::null_mut();
            let mut bytes_read: u64 = 0;
            let mut meta = ffi::RGWObjectMeta {
                size: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                etag: ptr::null_mut(),
            };

            // Determine offset and length from range
            let (offset, len) = match &range {
                Some(GetRange::Bounded(r)) => (r.start, r.end - r.start),
                Some(GetRange::Offset(offset)) => (*offset, 0),
                Some(GetRange::Suffix(_)) => (0, 0), // Simplified
                None => (0, 0),
            };

            // Call RGW GET API
            let ret = unsafe {
                ffi::rgw_get_object(
                    driver_ptr,
                    dpp_ptr,
                    bucket_c.as_ptr(),
                    key_c.as_ptr(),
                    offset,
                    len,
                    ptr::null_mut(), // conditionals
                    &mut buffer_ptr,
                    &mut bytes_read,
                    &mut meta,
                )
            };

            // Check for errors
            check_errno(ret, &location_clone)?;

            // Wrap buffer in RAII wrapper
            let buffer = unsafe { RgwBuffer::from_raw(buffer_ptr, bytes_read as usize) };
            let bytes = Bytes::copy_from_slice(buffer.as_bytes());

            // Extract metadata
            let etag = if !meta.etag.is_null() {
                let etag_wrapper = unsafe { RgwString::from_raw(meta.etag) };
                etag_wrapper.to_string()
            } else {
                None
            };

            let last_modified = Utc.timestamp_opt(meta.mtime_sec, meta.mtime_nsec as u32)
                .single()
                .unwrap_or_else(|| Utc::now());

            let obj_meta = ObjectMeta {
                location: location_clone.clone(),
                last_modified,
                size: meta.size,
                e_tag: etag,
                version: None,
            };

            Ok::<(Bytes, ObjectMeta), object_store::Error>((bytes, obj_meta))
        })
        .await
        .map_err(|e| object_store::Error::Generic {
            store: STORE,
            source: Box::new(e),
        })??;

        let actual_range = match options.range {
            Some(GetRange::Bounded(r)) => r.start..r.start + bytes.len() as u64,
            _ => 0..bytes.len() as u64,
        };

        Ok(GetResult {
            payload: GetResultPayload::Stream(Box::pin(stream::once(async move { Ok(bytes) }))),
            meta,
            range: actual_range,
            attributes: Default::default(),
        })
    }

    async fn delete(&self, location: &Path) -> OSResult<()> {
        let bucket = self.bucket_name.clone();
        let key = location.to_string();
        let driver_ptr = self.driver as usize;
        let dpp_ptr = self.dpp as usize;
        let location_clone = location.clone();

        tokio::task::spawn_blocking(move || {
            let driver_ptr = driver_ptr as *mut c_void;
            let dpp_ptr = dpp_ptr as *const c_void;

            let bucket_c = CString::new(bucket).map_err(|e| object_store::Error::Generic {
                store: STORE,
                source: Box::new(e),
            })?;
            let key_c = CString::new(key).map_err(|e| object_store::Error::Generic {
                store: STORE,
                source: Box::new(e),
            })?;

            let ret = unsafe {
                ffi::rgw_delete_object(driver_ptr, dpp_ptr, bucket_c.as_ptr(), key_c.as_ptr())
            };

            check_errno(ret, &location_clone)?;
            Ok::<(), object_store::Error>(())
        })
        .await
        .map_err(|e| object_store::Error::Generic {
            store: STORE,
            source: Box::new(e),
        })??;

        Ok(())
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, OSResult<ObjectMeta>> {
        let bucket = self.bucket_name.clone();
        let prefix_str = prefix.map(|p| p.to_string()).unwrap_or_default();
        let driver_ptr = self.driver as usize;
        let dpp_ptr = self.dpp as usize;

        let stream = stream::once(async move {
            tokio::task::spawn_blocking(move || {
                let driver_ptr = driver_ptr as *mut c_void;
                let dpp_ptr = dpp_ptr as *const c_void;

                let bucket_c = CString::new(bucket).map_err(|e| object_store::Error::Generic {
                    store: STORE,
                    source: Box::new(e),
                })?;
                let prefix_c = CString::new(prefix_str).map_err(|e| object_store::Error::Generic {
                    store: STORE,
                    source: Box::new(e),
                })?;

                let mut result = ffi::RGWListResult {
                    entries: ptr::null_mut(),
                    num_objects: 0,
                    common_prefixes: ptr::null_mut(),
                    num_common_prefixes: 0,
                    next_marker: ptr::null_mut(),
                    is_truncated: 0,
                };

                let ret = unsafe {
                    ffi::rgw_list_objects(
                        driver_ptr,
                        dpp_ptr,
                        bucket_c.as_ptr(),
                        prefix_c.as_ptr(),
                        ptr::null(), // delimiter
                        ptr::null(), // marker
                        1000,        // max_keys
                        &mut result,
                    )
                };

                if ret < 0 {
                    return Err(object_store::Error::Generic {
                        store: STORE,
                        source: format!("list failed with errno {}", ret).into(),
                    });
                }

                let wrapper = unsafe { RgwListResultWrapper::from_raw(result) };
                let entries = wrapper.entries();
                Ok::<Vec<ObjectMeta>, object_store::Error>(entries)
            })
            .await
            .map_err(|e| object_store::Error::Generic {
                store: STORE,
                source: Box::new(e),
            })?
        })
        .map(|result: Result<Vec<ObjectMeta>, object_store::Error>| {
            match result {
                Ok(entries) => stream::iter(entries.into_iter().map(Ok)).left_stream(),
                Err(e) => stream::once(async move { Err(e) }).right_stream(),
            }
        })
        .flatten()
        .boxed();

        stream
    }

    async fn list_with_delimiter(&self, _prefix: Option<&Path>) -> OSResult<ListResult> {
        Ok(ListResult {
            common_prefixes: vec![],
            objects: vec![],
        })
    }

    async fn copy(&self, _from: &Path, _to: &Path) -> OSResult<()> {
        Err(object_store::Error::NotImplemented)
    }

    async fn copy_if_not_exists(&self, _from: &Path, _to: &Path) -> OSResult<()> {
        Err(object_store::Error::NotImplemented)
    }
}
