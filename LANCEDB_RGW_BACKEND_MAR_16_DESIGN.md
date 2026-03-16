# LanceDB RGW Backend Design Document

**Date:** March 16, 2026
**Status:** Draft
**Author:** Engineering Team

---

## Overview

This document describes the integration that enables LanceDB to use Ceph RADOS Gateway (RGW) as a direct storage backend, bypassing the S3 protocol and accessing Ceph storage through the native SAL (Storage Abstraction Layer) C API.

### Goals

1. Enable LanceDB to store vector data directly in Ceph via RGW SAL
2. Avoid S3 protocol overhead for better performance
3. Maintain clean separation between components
4. Provide reusable RGW integration for multiple language bindings

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Ceph RGW Application                            │
│                    (Operation handlers, data services)                   │
└────────────┬────────────────────────────────────────────┬───────────────┘
             │                                            │
             │ Generic LanceDB C API                      │ RGW Store FFI
             │                                            │
        ┌────▼────────────────┐                    ┌─────▼──────────────┐
        │     lancedb-c       │                    │     lance-io       │
        │  (C/C++ bindings)   │                    │   (rgw feature)    │
        │                     │                    │                    │
        │ - Connection API    │                    │ - RGWObjectStore   │
        │ - Table API         │                    │ - FFI wrapper      │
        │ - Query API         │                    │ - SAL C bindings   │
        │ - Wrap callbacks    │                    │                    │
        └────────┬────────────┘                    └─────┬──────────────┘
                 │                                        │
                 └────────────────┬───────────────────────┘
                                  │
                           ┌──────▼───────┐
                           │   lancedb    │
                           │ (Rust core)  │
                           └──────┬───────┘
                                  │
                           ┌──────▼───────┐
                           │    lance     │
                           │ (format lib) │
                           └──────────────┘
```

---

## Repository Changes

### 1. lance (rust/lance-io)

#### New Files

| File | Purpose |
|------|---------|
| `rust/lance-io/src/object_store/rgw/mod.rs` | `RGWObjectStore` implementing `ObjectStore` trait |
| `rust/lance-io/src/object_store/rgw/ffi.rs` | FFI bindings to Ceph SAL C API |
| `rust/lance-io/src/object_store/rgw/ffi_wrapper.rs` | C FFI exports for external consumers |
| `rust/lance-io/src/object_store/rgw/error.rs` | Error handling and conversion |
| `rust/lance-io/src/object_store/rgw/memory.rs` | RAII wrappers for C-allocated memory |
| `rust/lance-io/src/object_store/providers/rgw.rs` | `ObjectStoreProvider` for `rgw://` URLs |
| `rust/lance-io/build.rs` | Build script for linking Ceph libraries |

#### Modified Files

| File | Changes |
|------|---------|
| `rust/lance-io/Cargo.toml` | Added `rgw` feature flag |

#### Feature Flag

```toml
[features]
rgw = []  # Enable RGW backend support
```

#### FFI Functions Exported

```c
// Create RGW store from driver/dpp pointers
LanceIOObjectStore* lance_io_create_rgw_store(
    void* driver,        // RGW driver (env.driver)
    const void* dpp,     // DoutPrefixProvider
    const char* bucket   // Bucket name
);

// Free the store
void lance_io_object_store_free(LanceIOObjectStore* store);
```

#### Ceph SAL C API Functions Used

```c
// Core operations
int rgw_put_object(driver, dpp, bucket, key, data, len, content_type, conds, etag);
int rgw_get_object(driver, dpp, bucket, key, offset, len, conds, buffer, bytes_read, meta);
int rgw_delete_object(driver, dpp, bucket, key);
int rgw_list_objects(driver, dpp, bucket, prefix, delimiter, marker, max, result);
int rgw_copy_object(driver, dpp, src_bucket, src_key, dst_bucket, dst_key);
int rgw_delete_objects(driver, dpp, bucket, keys, count);

// Range reads
int rgw_get_object_ranges(driver, dpp, bucket, key, ranges, count, results, result_count);

// Multipart uploads
int rgw_init_multipart(driver, dpp, bucket, key, upload_id);
int rgw_multipart_put_part(driver, dpp, bucket, key, upload_id, part_num, data, len, etag);
int rgw_multipart_complete(driver, dpp, bucket, key, upload_id, etags, count, final_etag);
int rgw_multipart_abort(driver, dpp, bucket, key, upload_id);

// Memory management
void rgw_free_buffer(buffer);
void rgw_free_string(str);
void rgw_list_result_free(result);
void rgw_object_meta_free(meta);
void rgw_free_ranges(results, count);
```

#### Linking Configuration (build.rs)

```rust
// Static libraries from Ceph build
cargo:rustc-link-lib=static=rgw_common
cargo:rustc-link-lib=static=rgw_a

// Dynamic libraries
cargo:rustc-link-lib=dylib=ceph-common
cargo:rustc-link-lib=dylib=rados
cargo:rustc-link-lib=dylib=stdc++
cargo:rustc-link-lib=dylib=crypto
cargo:rustc-link-lib=dylib=ssl
cargo:rustc-link-lib=dylib=pthread
cargo:rustc-link-lib=dylib=resolv
```

---

### 2. lancedb-c

**No RGW-specific code changes required!**

The integration uses the existing `WrappingObjectStore` callback mechanism:

```c
typedef struct {
    // ... other fields ...
    LanceDBWrapObjectStoreFn wrap_fn;     // Object store wrapper callback
    void* wrap_user_data;                  // User data for callback
    LanceDBFreeUserDataFn free_user_data;  // Cleanup callback
} LanceDBObjectStoreParams;
```

#### New Example File

| File | Purpose |
|------|---------|
| `examples/rgw.cpp` | Demonstrates RGW integration with null driver/dpp |

#### Documentation

| File | Purpose |
|------|---------|
| `RGW-INTEGRATION.md` | Architecture and usage guide |

---

### 3. ceph (proposed changes - not yet implemented)

#### New Files (to be added)

| File | Purpose |
|------|---------|
| `src/rgw/rgw_sal_c_wrapper.h` | C API header for SAL operations |
| `src/rgw/rgw_sal_c_wrapper.cc` | C wrapper implementation |

The C wrapper implements the `rgw_*` functions that `lance-io/ffi.rs` binds to.
These are built into `librgw_common.a` and `librgw_a.a` in the Ceph build.

---

## Data Flow

```
1. RGW Operation Handler
   │
   ├─► Get driver/dpp from RGW environment
   │   - driver = env.driver
   │   - dpp = this (DoutPrefixProvider)
   │
   ├─► Connect: lancedb_connect("rgw://bucket/path")
   │
   ├─► Create ObjectStoreParams with wrap callback
   │   - wrap_fn = wrap_with_rgw
   │   - wrap_user_data = {driver, dpp, bucket}
   │
   ├─► LanceDB operations (create table, insert, query)
   │   │
   │   └─► wrap_fn called → lance_io_create_rgw_store()
   │       │
   │       └─► RGWObjectStore created
   │           │
   │           └─► I/O operations call rgw_* C functions
   │               │
   │               └─► Ceph SAL layer
   │                   │
   │                   └─► RADOS
   │
   └─► Cleanup: lancedb_connection_free()
```

---

## Usage Example

### In an RGW Operation Handler

```cpp
#include <rgw/rgw_op.h>
#include "lancedb.h"

// Declare lance-io RGW FFI
extern "C" {
    struct LanceIOObjectStore;
    LanceIOObjectStore* lance_io_create_rgw_store(void*, const void*, const char*);
    void lance_io_object_store_free(LanceIOObjectStore*);
}

struct RGWConfig {
    void* driver;
    const void* dpp;
    const char* bucket;
};

LanceDBObjectStore* wrap_with_rgw(
    const LanceDBObjectStore* original,
    const char* const* keys,
    const char* const* values,
    size_t count,
    void* user_data)
{
    auto* cfg = static_cast<RGWConfig*>(user_data);
    return reinterpret_cast<LanceDBObjectStore*>(
        lance_io_create_rgw_store(cfg->driver, cfg->dpp, cfg->bucket)
    );
}

class RGWLanceDBOp : public RGWOp {
public:
    void execute() override {
        // Get RGW driver and DPP from environment
        RGWConfig config = {
            .driver = env.driver,              // From RGW request state
            .dpp = this,                       // RGWOp is a DoutPrefixProvider
            .bucket = s->bucket_name.c_str()
        };

        // Connect to LanceDB via RGW
        std::string uri = "rgw://" + s->bucket_name + "/vectors";
        auto* builder = lancedb_connect(uri.c_str());
        auto* db = lancedb_connect_builder_execute(builder);

        // Create table with RGW storage
        LanceDBObjectStoreParams store_params;
        lancedb_object_store_params_defaults(&store_params);
        store_params.wrap_fn = wrap_with_rgw;
        store_params.wrap_user_data = &config;

        LanceDBWriteOptions write_opts;
        lancedb_write_options_defaults(&write_opts);
        write_opts.store_params = &store_params;

        // Create table, add data, query...
        // (using standard LanceDB C API)

        lancedb_connection_free(db);
    }
};
```

---

## Build Instructions

```bash
# 1. Build Ceph (once)
cd ceph
./do_cmake.sh
cd build && ninja

# 2. Build lance-io with RGW support
cd lance
CEPH_PATH=/path/to/ceph cargo build --release -p lance-io --features rgw

# 3. Build lancedb-c
cd lancedb-c
cargo build --release

# 4. Link your application
g++ my_rgw_app.cpp \
    -I/path/to/lancedb-c/include \
    -L/path/to/lancedb-c/target/release -llancedb \
    -L/path/to/lance/target/release -llance_io \
    -L/path/to/ceph/build/lib -lceph-common -lrados -lstdc++ \
    -Wl,-rpath,/path/to/ceph/build/lib \
    -o my_rgw_app
```

---

## Test Results

### FFI Linking Tests (15 passed)

| Test | Result |
|------|--------|
| All 16+ SAL C API symbols linked | PASS |
| FFI struct sizes verified | PASS |
| FFI struct alignments verified | PASS |
| Null driver returns -22 (EINVAL) | PASS |
| Multipart init returns -38 (ENOSYS) | PASS |
| URL parsing for `rgw://` scheme | PASS |
| Memory wrappers handle null pointers | PASS |
| Provider tests (path extraction, prefix) | PASS |

### Test Commands

```bash
# Run all RGW tests
CEPH_PATH=/path/to/ceph cargo test -p lance-io --features rgw rgw:: -- --nocapture

# Run FFI symbol tests only
CEPH_PATH=/path/to/ceph cargo test -p lance-io --features rgw rgw::ffi::tests -- --nocapture
```

---

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| RGW code in lance-io, not lancedb-c | Clean separation; reusable by Python/Node bindings |
| Use wrap callback mechanism | No changes needed to lancedb-c; standard pattern |
| Static linking for rgw_common/rgw_a | Avoids ABI compatibility issues |
| Dynamic linking for ceph-common/rados | Reuses running Ceph's libraries |
| `rgw://` URL scheme | Consistent with other object store schemes (s3://, gs://) |
| Thread-safe RGWObjectStore | Required for async I/O; assumes driver is thread-safe |
| RAII wrappers for C memory | Prevents memory leaks; idiomatic Rust |
| Negative errno return codes | Consistent with Ceph C API conventions |

---

## Component Responsibilities

### lance-io (RGW module)

- Implements `ObjectStore` trait for RGW
- Provides C FFI for external consumers
- Handles memory management for C allocations
- Translates between Rust errors and errno codes

### lancedb-c

- Provides generic C/C++ bindings (no RGW knowledge)
- Exposes `ObjectStoreParams` with wrap callback
- Manages connection and table lifecycles

### Ceph (RGW SAL)

- Provides `rgw_*` C API functions
- Handles actual RADOS I/O
- Manages authentication and bucket access

---

## Future Work

- [ ] Add example in Ceph codebase (RGW operation handler)
- [ ] Implement multipart upload support (currently returns ENOSYS)
- [ ] Add RGW authentication via storage options
- [ ] Python/Node.js bindings for RGW backend
- [ ] Performance benchmarks vs S3 protocol
- [ ] Integration tests with real Ceph cluster
- [ ] Documentation for Ceph developers

---

## Appendix: File Locations

### lance repository

```
rust/lance-io/
├── Cargo.toml                          # Added 'rgw' feature
├── build.rs                            # NEW: Ceph linking configuration
└── src/object_store/
    ├── rgw/
    │   ├── mod.rs                      # NEW: RGWObjectStore
    │   ├── ffi.rs                      # NEW: Ceph SAL C bindings
    │   ├── ffi_wrapper.rs              # NEW: C FFI exports
    │   ├── error.rs                    # NEW: Error handling
    │   └── memory.rs                   # NEW: RAII wrappers
    └── providers/
        └── rgw.rs                      # NEW: ObjectStoreProvider
```

### lancedb-c repository

```
lancedb-c/
├── include/lancedb.h                   # Existing (unchanged)
├── RGW-INTEGRATION.md                  # NEW: Architecture doc
└── examples/
    └── rgw.cpp                         # NEW: Demo example
```

### ceph repository (proposed)

```
ceph/src/rgw/
├── rgw_sal_c_wrapper.h                 # NEW: C API header
└── rgw_sal_c_wrapper.cc                # NEW: C wrapper impl
```

---

## References

- [LanceDB Documentation](https://lancedb.github.io/lancedb/)
- [Ceph RGW Developer Guide](https://docs.ceph.com/en/latest/radosgw/)
- [Apache Arrow C Data Interface](https://arrow.apache.org/docs/format/CDataInterface.html)
- [Rust FFI Guide](https://doc.rust-lang.org/nomicon/ffi.html)
