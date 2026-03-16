// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
//   Unless required by applicable law or agreed to in writing,
//   software distributed under the License is distributed on an
//   "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
//   KIND, either express or implied.  See the License for the
//   specific language governing permissions and limitations
//   under the License.

//! FFI bindings to RGW SAL C API (rgw_sal_c.h)
//!
//! This module provides raw C bindings to the RGW Storage Abstraction Layer C API.
//! All functions and types are direct mappings to the C API defined in ceph/src/include/rgw/rgw_sal_c.h.

use std::os::raw::{c_char, c_int, c_void};

/// Conditionals for PUT operations
#[repr(C)]
#[derive(Debug)]
pub struct RGWPutConditionals {
    /// Only write if ETag matches this value
    pub if_match: *const c_char,
    /// Only write if ETag doesn't match this value
    pub if_none_match: *const c_char,
}

/// Conditionals for GET operations
#[repr(C)]
#[derive(Debug)]
pub struct RGWGetConditionals {
    /// Only return if ETag matches
    pub if_match: *const c_char,
    /// Only return if ETag doesn't match
    pub if_none_match: *const c_char,
    /// Only return if modified since this timestamp (Unix epoch seconds)
    pub if_modified_since: i64,
    /// Only return if not modified since this timestamp (Unix epoch seconds)
    pub if_unmodified_since: i64,
}

/// Object metadata returned by RGW
#[repr(C)]
#[derive(Debug)]
pub struct RGWObjectMeta {
    /// Object size in bytes
    pub size: u64,
    /// Modification time (seconds since Unix epoch)
    pub mtime_sec: i64,
    /// Modification time (nanoseconds)
    pub mtime_nsec: i64,
    /// ETag (caller must free with rgw_free_string)
    pub etag: *mut c_char,
}

/// Single entry in a list result
#[repr(C)]
#[derive(Debug)]
pub struct RGWObjectEntry {
    /// Object key (caller must free with rgw_free_string)
    pub key: *mut c_char,
    /// Object ETag (caller must free with rgw_free_string)
    pub etag: *mut c_char,
    /// Object size in bytes
    pub size: u64,
    /// Modification time (seconds since Unix epoch)
    pub mtime_sec: i64,
    /// Modification time (nanoseconds)
    pub mtime_nsec: i32,
}

/// Result of a list operation
#[repr(C)]
#[derive(Debug)]
pub struct RGWListResult {
    /// Array of object entries (caller must free with rgw_list_result_free)
    pub entries: *mut RGWObjectEntry,
    /// Number of objects in entries array
    pub num_objects: u32,
    /// Array of common prefixes for hierarchical listing (caller must free with rgw_list_result_free)
    pub common_prefixes: *mut *mut c_char,
    /// Number of common prefixes
    pub num_common_prefixes: u32,
    /// Marker for next page of results (caller must free with rgw_free_string)
    pub next_marker: *mut c_char,
    /// Non-zero if more results available
    pub is_truncated: i32,
}

/// Byte range for multi-range GET
#[repr(C)]
#[derive(Debug)]
pub struct RGWRange {
    /// Start byte offset (inclusive)
    pub start: u64,
    /// End byte offset (inclusive)
    pub end: u64,
}

/// Result for a single range read
#[repr(C)]
#[derive(Debug)]
pub struct RGWRangeResult {
    /// Data buffer (caller must free with rgw_free_buffer)
    pub data: *mut c_char,
    /// Length of data
    pub len: u64,
}

// Link to librgw library
// Linking is now handled by build.rs when the 'rgw' feature is enabled.
// The build.rs script will:
// 1. Use the CEPH_PATH environment variable (or default to ../ceph)
// 2. Link against librgw_common.a and librgw_a.a from the local ceph build
// 3. Link against required dependencies (libceph-common, librados, etc.)
// 4. Set up rpath so libraries can be found at runtime
//
// To build with RGW support:
//   CEPH_PATH=/path/to/ceph cargo build --features rgw
unsafe extern "C" {
    //
    // Core object operations
    //

    /// PUT object to RGW
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket and key must be valid null-terminated C strings
    /// - data must be valid for data_len bytes
    /// - If etag is non-null, caller must free the returned string with rgw_free_string
    ///
    /// # Returns
    /// 0 on success, negative errno on failure
    pub fn rgw_put_object(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        key: *const c_char,
        data: *const c_char,
        data_len: u64,
        obj_attributes: *mut c_void,
        conds: *const RGWPutConditionals,
        etag: *mut *mut c_char,
    ) -> c_int;

    /// GET object from RGW
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket and key must be valid null-terminated C strings
    /// - buffer will be allocated by RGW, caller must free with rgw_free_buffer
    /// - If meta is non-null and meta.etag is non-null, caller must free with rgw_free_string
    ///
    /// # Returns
    /// 0 on success, negative errno on failure
    pub fn rgw_get_object(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        key: *const c_char,
        offset: u64,
        len: u64,
        conds: *mut RGWGetConditionals,
        buffer: *mut *mut c_char,
        bytes_read: *mut u64,
        meta: *mut RGWObjectMeta,
    ) -> c_int;

    /// DELETE object from RGW
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket and key must be valid null-terminated C strings
    ///
    /// # Returns
    /// 0 on success, negative errno on failure
    pub fn rgw_delete_object(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        key: *const c_char,
    ) -> c_int;

    /// LIST objects in RGW bucket
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket must be a valid null-terminated C string
    /// - prefix, delimiter, marker can be null or valid null-terminated C strings
    /// - result will be populated by RGW, caller must free with rgw_list_result_free
    ///
    /// # Returns
    /// 0 on success, negative errno on failure
    pub fn rgw_list_objects(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        prefix: *const c_char,
        delimiter: *const c_char,
        marker: *const c_char,
        max_keys: c_int,
        result: *mut RGWListResult,
    ) -> c_int;

    /// COPY object within or across buckets
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - All bucket and key parameters must be valid null-terminated C strings
    ///
    /// # Returns
    /// 0 on success, negative errno on failure (currently returns -ENOSYS)
    pub fn rgw_copy_object(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        src_bucket: *const c_char,
        src_key: *const c_char,
        dst_bucket: *const c_char,
        dst_key: *const c_char,
    ) -> c_int;

    /// DELETE multiple objects in a single operation
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket must be a valid null-terminated C string
    /// - keys must be an array of num_keys valid null-terminated C strings
    ///
    /// # Returns
    /// 0 if all deletes succeeded, negative errno if any failed
    pub fn rgw_delete_objects(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        keys: *const *const c_char,
        num_keys: u32,
    ) -> c_int;

    /// GET multiple byte ranges from an object
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket and key must be valid null-terminated C strings
    /// - ranges must be an array of num_ranges RGWRange structs
    /// - results will be allocated by RGW, caller must free with rgw_free_ranges
    ///
    /// # Returns
    /// 0 on success, negative errno on failure
    pub fn rgw_get_object_ranges(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        key: *const c_char,
        ranges: *const RGWRange,
        num_ranges: u32,
        results: *mut *mut RGWRangeResult,
        result_count: *mut u32,
    ) -> c_int;

    //
    // Multipart upload operations
    //

    /// Initialize multipart upload
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket and key must be valid null-terminated C strings
    /// - upload_id will be allocated by RGW, caller must free with rgw_free_string
    ///
    /// # Returns
    /// 0 on success, negative errno on failure
    pub fn rgw_init_multipart(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        key: *const c_char,
        upload_id: *mut *mut c_char,
    ) -> c_int;

    /// Upload a part in multipart upload
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket, key, upload_id must be valid null-terminated C strings
    /// - data must be valid for len bytes
    /// - etag will be allocated by RGW, caller must free with rgw_free_string
    ///
    /// # Returns
    /// 0 on success, negative errno on failure
    pub fn rgw_multipart_put_part(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        key: *const c_char,
        upload_id: *const c_char,
        part_num: u64,
        data: *const c_char,
        len: u64,
        etag: *mut *mut c_char,
    ) -> c_int;

    /// Complete multipart upload
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket, key, upload_id must be valid null-terminated C strings
    /// - part_etags must be an array of num_parts valid null-terminated C strings
    /// - final_etag will be allocated by RGW, caller must free with rgw_free_string
    ///
    /// # Returns
    /// 0 on success, negative errno on failure
    pub fn rgw_multipart_complete(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        key: *const c_char,
        upload_id: *const c_char,
        part_etags: *const *const c_char,
        num_parts: u32,
        final_etag: *mut *mut c_char,
    ) -> c_int;

    /// Abort multipart upload
    ///
    /// # Safety
    /// - driver_ptr and dpp_ptr must be valid RGW Driver and DPP pointers
    /// - bucket, key, upload_id must be valid null-terminated C strings
    ///
    /// # Returns
    /// 0 on success, negative errno on failure
    pub fn rgw_multipart_abort(
        driver_ptr: *mut c_void,
        dpp_ptr: *const c_void,
        bucket: *const c_char,
        key: *const c_char,
        upload_id: *const c_char,
    ) -> c_int;

    //
    // Memory management functions
    //

    /// Free buffer allocated by RGW
    ///
    /// # Safety
    /// - buffer must have been allocated by an RGW function (e.g., rgw_get_object)
    /// - buffer must not be used after this call
    pub fn rgw_free_buffer(buffer: *mut c_char);

    /// Free string allocated by RGW
    ///
    /// # Safety
    /// - str must have been allocated by an RGW function (e.g., etag returns)
    /// - str must not be used after this call
    pub fn rgw_free_string(str: *mut c_char);

    /// Free list result structure
    ///
    /// # Safety
    /// - result must have been populated by rgw_list_objects
    /// - result must not be used after this call
    pub fn rgw_list_result_free(result: *mut RGWListResult);

    /// Free object metadata
    ///
    /// # Safety
    /// - meta must have been populated by an RGW function
    /// - meta must not be used after this call
    pub fn rgw_object_meta_free(meta: *mut RGWObjectMeta);

    /// Free range results
    ///
    /// # Safety
    /// - results must have been allocated by rgw_get_object_ranges
    /// - results must not be used after this call
    pub fn rgw_free_ranges(results: *mut RGWRangeResult, num_ranges: u32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::ptr;

    /// Test that all FFI symbols are linked correctly.
    /// This test verifies the linker can resolve all RGW SAL C API symbols.
    /// It doesn't call the functions (that would require a real driver),
    /// but ensures the symbols exist in the linked libraries.
    #[test]
    fn test_ffi_symbols_linked() {
        // Get function pointers to verify symbols are linked
        // This will fail at link time if symbols are missing
        let _put_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            u64,
            *mut c_void,
            *const RGWPutConditionals,
            *mut *mut c_char,
        ) -> c_int = rgw_put_object;

        let _get_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
            u64,
            u64,
            *mut RGWGetConditionals,
            *mut *mut c_char,
            *mut u64,
            *mut RGWObjectMeta,
        ) -> c_int = rgw_get_object;

        let _delete_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
        ) -> c_int = rgw_delete_object;

        let _list_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
            c_int,
            *mut RGWListResult,
        ) -> c_int = rgw_list_objects;

        let _copy_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
        ) -> c_int = rgw_copy_object;

        let _delete_multi_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const *const c_char,
            u32,
        ) -> c_int = rgw_delete_objects;

        let _get_ranges_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
            *const RGWRange,
            u32,
            *mut *mut RGWRangeResult,
            *mut u32,
        ) -> c_int = rgw_get_object_ranges;

        // Multipart operations
        let _init_multipart_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
            *mut *mut c_char,
        ) -> c_int = rgw_init_multipart;

        let _put_part_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            u64,
            *const c_char,
            u64,
            *mut *mut c_char,
        ) -> c_int = rgw_multipart_put_part;

        let _complete_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
            *const c_char,
            *const *const c_char,
            u32,
            *mut *mut c_char,
        ) -> c_int = rgw_multipart_complete;

        let _abort_fn: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const c_char,
            *const c_char,
            *const c_char,
        ) -> c_int = rgw_multipart_abort;

        // Memory management
        let _free_buffer_fn: unsafe extern "C" fn(*mut c_char) = rgw_free_buffer;
        let _free_string_fn: unsafe extern "C" fn(*mut c_char) = rgw_free_string;
        let _list_result_free_fn: unsafe extern "C" fn(*mut RGWListResult) = rgw_list_result_free;
        let _meta_free_fn: unsafe extern "C" fn(*mut RGWObjectMeta) = rgw_object_meta_free;
        let _free_ranges_fn: unsafe extern "C" fn(*mut RGWRangeResult, u32) = rgw_free_ranges;

        // If we get here, all symbols are linked
        println!("All RGW SAL C API symbols successfully linked!");
    }

    /// Test that FFI struct layouts match expected sizes.
    /// This helps catch ABI mismatches between Rust and C.
    #[test]
    fn test_ffi_struct_sizes() {
        // RGWPutConditionals: 2 pointers
        assert_eq!(
            std::mem::size_of::<RGWPutConditionals>(),
            2 * std::mem::size_of::<*const c_char>(),
            "RGWPutConditionals size mismatch"
        );

        // RGWGetConditionals: 2 pointers + 2 i64s
        assert_eq!(
            std::mem::size_of::<RGWGetConditionals>(),
            2 * std::mem::size_of::<*const c_char>() + 2 * std::mem::size_of::<i64>(),
            "RGWGetConditionals size mismatch"
        );

        // RGWObjectMeta: u64 + i64 + i64 + pointer
        assert_eq!(
            std::mem::size_of::<RGWObjectMeta>(),
            std::mem::size_of::<u64>()
                + std::mem::size_of::<i64>()
                + std::mem::size_of::<i64>()
                + std::mem::size_of::<*mut c_char>(),
            "RGWObjectMeta size mismatch"
        );

        // RGWRange: 2 u64s
        assert_eq!(
            std::mem::size_of::<RGWRange>(),
            2 * std::mem::size_of::<u64>(),
            "RGWRange size mismatch"
        );

        // RGWRangeResult: pointer + u64
        assert_eq!(
            std::mem::size_of::<RGWRangeResult>(),
            std::mem::size_of::<*mut c_char>() + std::mem::size_of::<u64>(),
            "RGWRangeResult size mismatch"
        );

        println!("All FFI struct sizes match expected values!");
    }

    /// Test that struct alignment is correct for FFI.
    #[test]
    fn test_ffi_struct_alignment() {
        // All structs should have pointer alignment (8 bytes on 64-bit)
        assert!(
            std::mem::align_of::<RGWPutConditionals>() >= std::mem::align_of::<*const c_void>(),
            "RGWPutConditionals alignment too small"
        );
        assert!(
            std::mem::align_of::<RGWGetConditionals>() >= std::mem::align_of::<*const c_void>(),
            "RGWGetConditionals alignment too small"
        );
        assert!(
            std::mem::align_of::<RGWObjectMeta>() >= std::mem::align_of::<*const c_void>(),
            "RGWObjectMeta alignment too small"
        );
        assert!(
            std::mem::align_of::<RGWObjectEntry>() >= std::mem::align_of::<*const c_void>(),
            "RGWObjectEntry alignment too small"
        );
        assert!(
            std::mem::align_of::<RGWListResult>() >= std::mem::align_of::<*const c_void>(),
            "RGWListResult alignment too small"
        );

        println!("All FFI struct alignments are correct!");
    }

    /// Test that null driver/dpp pointers are handled (returns error).
    /// This tests that the C API properly validates inputs.
    #[test]
    fn test_null_driver_returns_error() {
        let bucket = CString::new("test-bucket").unwrap();
        let key = CString::new("test-key").unwrap();
        let data = b"test data";

        let mut etag_ptr: *mut c_char = ptr::null_mut();

        // Call with null driver - should return error (negative errno)
        let ret = unsafe {
            rgw_put_object(
                ptr::null_mut(), // null driver
                ptr::null(),     // null dpp
                bucket.as_ptr(),
                key.as_ptr(),
                data.as_ptr() as *const c_char,
                data.len() as u64,
                ptr::null_mut(),
                ptr::null(),
                &mut etag_ptr,
            )
        };

        // Should return negative errno (EINVAL or similar)
        assert!(
            ret < 0,
            "Expected negative errno for null driver, got {}",
            ret
        );
        println!(
            "Null driver correctly rejected with errno: {}",
            ret
        );
    }

    /// Test GET with null driver returns error.
    #[test]
    fn test_get_null_driver_returns_error() {
        let bucket = CString::new("test-bucket").unwrap();
        let key = CString::new("test-key").unwrap();

        let mut buffer_ptr: *mut c_char = ptr::null_mut();
        let mut bytes_read: u64 = 0;
        let mut meta = RGWObjectMeta {
            size: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            etag: ptr::null_mut(),
        };

        let ret = unsafe {
            rgw_get_object(
                ptr::null_mut(), // null driver
                ptr::null(),     // null dpp
                bucket.as_ptr(),
                key.as_ptr(),
                0,  // offset
                0,  // len (0 = read all)
                ptr::null_mut(),
                &mut buffer_ptr,
                &mut bytes_read,
                &mut meta,
            )
        };

        assert!(
            ret < 0,
            "Expected negative errno for null driver GET, got {}",
            ret
        );
        println!(
            "Null driver GET correctly rejected with errno: {}",
            ret
        );
    }

    /// Test DELETE with null driver returns error.
    #[test]
    fn test_delete_null_driver_returns_error() {
        let bucket = CString::new("test-bucket").unwrap();
        let key = CString::new("test-key").unwrap();

        let ret = unsafe {
            rgw_delete_object(
                ptr::null_mut(), // null driver
                ptr::null(),     // null dpp
                bucket.as_ptr(),
                key.as_ptr(),
            )
        };

        assert!(
            ret < 0,
            "Expected negative errno for null driver DELETE, got {}",
            ret
        );
        println!(
            "Null driver DELETE correctly rejected with errno: {}",
            ret
        );
    }

    /// Test LIST with null driver returns error.
    #[test]
    fn test_list_null_driver_returns_error() {
        let bucket = CString::new("test-bucket").unwrap();
        let prefix = CString::new("").unwrap();

        let mut result = RGWListResult {
            entries: ptr::null_mut(),
            num_objects: 0,
            common_prefixes: ptr::null_mut(),
            num_common_prefixes: 0,
            next_marker: ptr::null_mut(),
            is_truncated: 0,
        };

        let ret = unsafe {
            rgw_list_objects(
                ptr::null_mut(), // null driver
                ptr::null(),     // null dpp
                bucket.as_ptr(),
                prefix.as_ptr(),
                ptr::null(), // delimiter
                ptr::null(), // marker
                1000,
                &mut result,
            )
        };

        assert!(
            ret < 0,
            "Expected negative errno for null driver LIST, got {}",
            ret
        );
        println!(
            "Null driver LIST correctly rejected with errno: {}",
            ret
        );
    }

    /// Test multipart init with null driver returns error.
    #[test]
    fn test_multipart_init_null_driver_returns_error() {
        let bucket = CString::new("test-bucket").unwrap();
        let key = CString::new("test-key").unwrap();

        let mut upload_id: *mut c_char = ptr::null_mut();

        let ret = unsafe {
            rgw_init_multipart(
                ptr::null_mut(), // null driver
                ptr::null(),     // null dpp
                bucket.as_ptr(),
                key.as_ptr(),
                &mut upload_id,
            )
        };

        assert!(
            ret < 0,
            "Expected negative errno for null driver multipart init, got {}",
            ret
        );
        println!(
            "Null driver multipart init correctly rejected with errno: {}",
            ret
        );
    }
}
