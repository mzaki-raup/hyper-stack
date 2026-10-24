//! Object storage for the S3 tool (docs/tools/s3.md §4): a SigV4 signer and
//! an S3 client on `reqwest`, usable with AWS and S3-compatible stores.

pub mod http_profile;
pub mod nexus;
pub mod profile;
pub mod s3;
pub mod sigv4;
pub mod store;
pub mod webdav;
pub mod xml;

pub use store::{Backend, Caps, ObjectStore};
