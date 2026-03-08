// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: Copyright The Lance Authors

//! Error handling and conversion for RGW backend
//!
//! This module maps errno codes returned by the RGW C API to lance_core::Error.

use std::os::raw::c_int;
use std::ffi::NulError;
use lance_core::Error;
use object_store::path::Path;
use snafu::location;

const STORE: &str = "RGW";

/// RGW-specific error types
#[derive(Debug, thiserror::Error)]
pub enum RgwError {
    /// RGW operation failed with errno code
    #[error("RGW operation failed with errno {0}")]
    Errno(c_int),

    /// Invalid UTF-8 in C string
    #[error("Invalid UTF-8 in C string")]
    InvalidUtf8,

    /// Null pointer returned from RGW
    #[error("Null pointer returned from RGW")]
    NullPointer,

    /// Required field is missing
    #[error("Required field is missing: {0}")]
    MissingField(&'static str),

    /// String contains null byte
    #[error("String contains null byte")]
    NulError(#[from] NulError),

    /// Task join error
    #[error("Background task failed")]
    JoinError(#[from] tokio::task::JoinError),
}

impl RgwError {
    /// Convert errno code to RgwError
    pub fn from_errno(errno: c_int) -> Self {
        Self::Errno(errno)
    }

    /// Check if this is a "not found" error (ENOENT = -2)
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Errno(-2))
    }

    /// Check if this is an "already exists" error (EEXIST = -17)
    pub fn is_already_exists(&self) -> bool {
        matches!(self, Self::Errno(-17))
    }

    /// Get errno code if this is an Errno variant
    pub fn errno(&self) -> Option<c_int> {
        match self {
            Self::Errno(code) => Some(*code),
            _ => None,
        }
    }
}

impl From<RgwError> for Error {
    fn from(err: RgwError) -> Self {
        match err {
            RgwError::Errno(errno) => errno_to_error(errno, &Path::from("")),
            _ => Error::IO {
                source: err.to_string().into(),
                location: location!(),
            },
        }
    }
}

/// Convert errno code to lance_core::Error
///
/// Maps common errno values to appropriate Error variants.
pub fn errno_to_error(errno: c_int, path: &Path) -> Error {
    let path_str = path.to_string();
    match -errno {
        // ENOENT (2): No such file or directory
        2 => Error::NotFound {
            uri: path_str,
            location: location!(),
        },

        // EEXIST (17): File exists
        17 => Error::IO {
            source: format!("File already exists: {} (errno {})", path_str, errno).into(),
            location: location!(),
        },

        // EINVAL (22): Invalid argument
        22 => Error::invalid_input(format!("Invalid argument: {} (errno {})", path_str, errno)),

        // EACCES (13): Permission denied
        // EPERM (1): Operation not permitted
        1 | 13 => Error::IO {
            source: format!("Permission denied: {} (errno {})", path_str, errno).into(),
            location: location!(),
        },

        // ENOMEM (12): Out of memory
        12 => Error::IO {
            source: format!("Out of memory (errno {})", errno).into(),
            location: location!(),
        },

        // EIO (5): I/O error
        5 => Error::IO {
            source: format!("I/O error: {} (errno {})", path_str, errno).into(),
            location: location!(),
        },

        // All other errors
        _ => Error::IO {
            source: format!("RGW operation failed: {} (errno {})", path_str, errno).into(),
            location: location!(),
        },
    }
}

/// Check errno and convert to Result
///
/// Returns Ok(()) if errno is 0, otherwise returns Err with the errno mapped to Error.
pub fn check_errno(errno: c_int, path: &Path) -> lance_core::Result<()> {
    if errno < 0 {
        Err(errno_to_error(errno, path))
    } else {
        Ok(())
    }
}
