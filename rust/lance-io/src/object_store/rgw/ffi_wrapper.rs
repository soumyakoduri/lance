// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: Copyright The Lance Authors

//! C FFI wrapper for RGW ObjectStore
//!
//! Provides C-compatible functions to create RGWObjectStore instances
//! that can be used from C/C++ applications via WrappingObjectStore.

use std::ffi::{c_char, c_void, CStr};
use std::ptr;
use std::sync::Arc;

use object_store::ObjectStore as OSObjectStore;

use super::RGWObjectStore;

/// Opaque handle to an Arc<dyn ObjectStore> for C FFI
#[repr(C)]
pub struct LanceIOObjectStore {
    pub(crate) inner: Arc<dyn OSObjectStore>,
}

/// Create an RGW object store for C FFI
///
/// Creates an RGWObjectStore that implements both ObjectStore and WrappingObjectStore.
/// The returned handle can be used with LanceDB's ObjectStoreParams.
///
/// # Safety
/// - `driver` must be a valid pointer to an initialized RGW Driver
/// - `dpp` can be null or must be a valid pointer to a DoutPrefixProvider
/// - `bucket` must be a valid null-terminated C string
/// - Both driver and dpp pointers must remain valid for the lifetime of the object store
/// - The driver must be thread-safe
///
/// # Returns
/// - Pointer to LanceIOObjectStore on success
/// - Null pointer on failure
///
/// # Example (C++)
///
/// ```cpp
/// // From RGW application context with driver/dpp:
/// LanceIOObjectStore* rgw_store = lance_io_create_rgw_store(
///     driver_ptr,  // from env.driver
///     dpp_ptr,     // from request context (this)
///     "my-bucket"
/// );
///
/// if (rgw_store != NULL) {
///     // Use in LanceDB ObjectStoreParams or connection
/// }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lance_io_create_rgw_store(
    driver: *mut c_void,
    dpp: *const c_void,
    bucket: *const c_char,
) -> *mut LanceIOObjectStore {
    if driver.is_null() {
        eprintln!("Error: RGW driver pointer is null");
        return ptr::null_mut();
    }

    if bucket.is_null() {
        eprintln!("Error: bucket name is null");
        return ptr::null_mut();
    }

    let bucket_str = match CStr::from_ptr(bucket).to_str() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Invalid bucket name: {}", e);
            return ptr::null_mut();
        }
    };

    // Create RGW object store
    // RGWObjectStore implements both ObjectStore and WrappingObjectStore
    let rgw_store = RGWObjectStore::new(bucket_str.to_string(), driver, dpp);

    // Wrap in Arc<dyn ObjectStore> and then in LanceIOObjectStore
    let store_arc = Arc::new(rgw_store) as Arc<dyn OSObjectStore>;
    let wrapped = LanceIOObjectStore { inner: store_arc };

    Box::into_raw(Box::new(wrapped))
}

/// Free an object store created by lance_io_create_rgw_store
///
/// # Safety
/// - `store` must be a valid pointer returned by lance_io_create_rgw_store
/// - `store` must not be used after calling this function
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lance_io_object_store_free(store: *mut LanceIOObjectStore) {
    if !store.is_null() {
        let _ = Box::from_raw(store);
    }
}
