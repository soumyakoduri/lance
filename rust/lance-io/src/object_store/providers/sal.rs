// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: Copyright The Lance Authors

use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::object_store::{
    ObjectStore, ObjectStoreParams, ObjectStoreProvider, StorageOptions,
    DEFAULT_CLOUD_BLOCK_SIZE, DEFAULT_CLOUD_IO_PARALLELISM, DEFAULT_MAX_IOP_SIZE,
};
use lance_core::error::Result;
use object_store::{path::Path, RetryConfig};
use snafu::location;
use url::Url;

#[derive(Default, Debug)]
pub struct SalStoreProvider;

#[async_trait::async_trait]
impl ObjectStoreProvider for SalStoreProvider {
    async fn new_store(&self, base_path: Url, params: &ObjectStoreParams) -> Result<ObjectStore> {
        let bucket = base_path.host_str().ok_or_else(|| {
            lance_core::error::Error::invalid_input(
                "SAL URL must contain bucket name (sal://bucket/path)",
                location!(),
            )
        })?;

        let storage_options = StorageOptions(params.storage_options.clone().unwrap_or_default());
        let max_retries = storage_options.client_max_retries();
        let retry_timeout = storage_options.client_retry_timeout();

        let retry_config = RetryConfig {
            backoff: Default::default(),
            max_retries,
            retry_timeout: Duration::from_secs(retry_timeout),
        };

        // Build SAL object store using the builder pattern
        use object_store::rgw_sal::SalBuilder;

        let mut builder = SalBuilder::new().with_bucket(bucket).with_retry(retry_config);

        // Apply storage options for Ceph configuration
        if let Some(cluster) = storage_options.0.get("cluster_name") {
            builder = builder.with_cluster_name(cluster);
        }
        if let Some(user) = storage_options.0.get("user_name") {
            builder = builder.with_user_name(user);
        }
        if let Some(conf) = storage_options.0.get("conf_file") {
            builder = builder.with_conf_file(conf);
        }

        let store = builder.build().map_err(|e| {
            lance_core::error::Error::invalid_input(
                format!("Failed to create SAL object store: {}", e),
                location!(),
            )
        })?;

        let block_size = params.block_size.unwrap_or(DEFAULT_CLOUD_BLOCK_SIZE);
        let download_retry_count = storage_options.download_retry_count();

        Ok(ObjectStore {
            inner: Arc::new(store),
            scheme: String::from("sal"),
            block_size,
            max_iop_size: *DEFAULT_MAX_IOP_SIZE,
            use_constant_size_upload_parts: false,
            list_is_lexically_ordered: true,
            io_parallelism: DEFAULT_CLOUD_IO_PARALLELISM,
            download_retry_count,
            io_tracker: Default::default(),
            store_prefix: self
                .calculate_object_store_prefix(&base_path, params.storage_options.as_ref())?,
        })
    }

    fn extract_path(&self, url: &Url) -> Result<Path> {
        // SAL paths are relative to bucket, similar to S3
        // URL format: sal://bucket/path/to/file -> path/to/file
        Path::parse(url.path().trim_start_matches('/')).map_err(|_| {
            lance_core::error::Error::invalid_input(
                format!("Invalid path in SAL URL: {}", url.path()),
                location!(),
            )
        })
    }

    fn calculate_object_store_prefix(
        &self,
        url: &Url,
        _storage_options: Option<&HashMap<String, String>>,
    ) -> Result<String> {
        // Format: sal$bucket_name
        // This uniquely identifies the SAL object store instance
        Ok(format!("{}${}", url.scheme(), url.authority()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sal_store_path_extraction() {
        let provider = SalStoreProvider;

        let url = Url::parse("sal://my-bucket/path/to/file").unwrap();
        let path = provider.extract_path(&url).unwrap();
        let expected_path = Path::from("path/to/file");
        assert_eq!(path, expected_path);

        // Test root path
        let url = Url::parse("sal://my-bucket/").unwrap();
        let path = provider.extract_path(&url).unwrap();
        let expected_path = Path::from("");
        assert_eq!(path, expected_path);

        // Test nested path
        let url = Url::parse("sal://my-bucket/deep/nested/path/file.lance").unwrap();
        let path = provider.extract_path(&url).unwrap();
        let expected_path = Path::from("deep/nested/path/file.lance");
        assert_eq!(path, expected_path);
    }

    #[test]
    fn test_calculate_object_store_prefix() {
        let provider = SalStoreProvider;

        let url = Url::parse("sal://my-bucket/path").unwrap();
        assert_eq!(
            "sal$my-bucket",
            provider
                .calculate_object_store_prefix(&url, None)
                .unwrap()
        );

        // Different bucket should have different prefix
        let url2 = Url::parse("sal://another-bucket/path").unwrap();
        assert_eq!(
            "sal$another-bucket",
            provider
                .calculate_object_store_prefix(&url2, None)
                .unwrap()
        );
    }
}
