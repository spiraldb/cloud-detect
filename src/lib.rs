//! # Cloud Detect
//!
//! A library to detect the cloud service provider of a host.
//!
//! ## Usage
//!
//! Add the following to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! # ...
//! cloud_detect = "2"
//! tokio = { version = "1", features = ["full"] }
//! tracing-subscriber = { version = "0.3", features = ["env-filter"] } # Optional; for logging
//! ```
//!
//! ## Examples
//!
//! Detect the cloud provider and print the result (with default timeout).
//!
//! ```rust
//! use cloud_detect::detect;
//!
//! #[tokio::main]
//! async fn main() {
//!     tracing_subscriber::fmt::init(); // Optional; for logging
//!
//!     let provider = detect(None).await;
//!     println!("Detected provider: {}", provider);
//! }
//! ```
//!
//! Detect the cloud provider and print the result (with custom timeout).
//!
//! ```rust
//! use std::time::Duration;
//!
//! use cloud_detect::detect;
//!
//! #[tokio::main]
//! async fn main() {
//!     tracing_subscriber::fmt::init(); // Optional; for logging
//!
//!     let provider = detect(Some(Duration::from_secs(10))).await;
//!     println!("Detected provider: {}", provider);
//! }
//! ```

use std::fmt::Debug;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use async_trait::async_trait;
use strum::Display;
use tokio::sync::mpsc::Sender;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinSet;

use crate::providers::*;

#[cfg(feature = "blocking")]
pub mod blocking;
pub(crate) mod providers;

/// Maximum time allowed for detection.
pub const DEFAULT_DETECTION_TIMEOUT: Duration = Duration::from_secs(5);

/// Represents an identifier for a cloud service provider.
#[non_exhaustive]
#[derive(Debug, Default, Display, Eq, PartialEq)]
pub enum ProviderId {
    /// Unknown cloud service provider.
    #[default]
    #[strum(serialize = "unknown")]
    Unknown,
    /// Akamai Cloud.
    #[strum(serialize = "akamai")]
    Akamai,
    /// Alibaba Cloud.
    #[strum(serialize = "alibaba")]
    Alibaba,
    /// Amazon Web Services (AWS).
    #[strum(serialize = "aws")]
    AWS,
    /// Microsoft Azure.
    #[strum(serialize = "azure")]
    Azure,
    /// DigitalOcean.
    #[strum(serialize = "digitalocean")]
    DigitalOcean,
    /// Google Cloud Platform (GCP).
    #[strum(serialize = "gcp")]
    GCP,
    /// Oracle Cloud Infrastructure (OCI).
    #[strum(serialize = "oci")]
    OCI,
    /// OpenStack.
    #[strum(serialize = "openstack")]
    OpenStack,
    /// Vultr.
    #[strum(serialize = "vultr")]
    Vultr,
}

/// Represents a cloud service provider.
#[async_trait]
pub(crate) trait Provider: Send + Sync {
    fn identifier(&self) -> ProviderId;
    async fn identify(&self, tx: Sender<ProviderId>);
}

type P = Arc<dyn Provider>;

static PROVIDERS: LazyLock<Vec<P>> = LazyLock::new(|| {
    vec![
        #[cfg(feature = "akami")]
        {
            Arc::new(akamai::Akamai) as P
        },
        #[cfg(feature = "alibaba")]
        {
            Arc::new(alibaba::Alibaba) as P
        },
        #[cfg(feature = "aws")]
        {
            Arc::new(aws::Aws) as P
        },
        #[cfg(feature = "azure")]
        {
            Arc::new(azure::Azure) as P
        },
        #[cfg(feature = "digitalocean")]
        {
            Arc::new(digitalocean::DigitalOcean) as P
        },
        #[cfg(feature = "gcp")]
        {
            Arc::new(gcp::Gcp) as P
        },
        #[cfg(feature = "oci")]
        {
            Arc::new(oci::Oci) as P
        },
        #[cfg(feature = "openstack")]
        {
            Arc::new(openstack::OpenStack) as P
        },
        #[cfg(feature = "vultr")]
        {
            Arc::new(vultr::Vultr) as P
        },
    ]
});

/// Returns a list of currently supported providers.
///
/// # Examples
///
/// Print the list of supported providers.
///
/// ```
/// use cloud_detect::supported_providers;
///
/// #[tokio::main]
/// async fn main() {
///     let providers = supported_providers().await;
///     println!("Supported providers: {:?}", providers);
/// }
/// ```
pub async fn supported_providers() -> Vec<String> {
    let providers: Vec<String> = PROVIDERS
        .iter()
        .map(|p| p.identifier().to_string())
        .collect();

    providers
}

/// Detects the host's cloud provider with a timeout, return `None` if all operations timed out.
pub async fn detect_with_timeout(duration: Duration) -> Option<ProviderId> {
    tokio::time::timeout(duration, detect()).await.ok()
}

/// Detects the host's cloud provider.
/// ```
pub async fn detect() -> ProviderId {
    let (tx, mut rx) = mpsc::channel::<ProviderId>(1);

    let provider_entries: Vec<P> = PROVIDERS.iter().cloned().collect();
    let providers_count = provider_entries.len();
    let mut handles = Vec::with_capacity(providers_count);

    // Create a counter that will be decremented as tasks complete
    let counter = Arc::new(AtomicUsize::new(providers_count));
    let complete = Arc::new(Notify::new());

    let mut join_set = JoinSet::new();

    for provider in provider_entries {
        let tx = tx.clone();
        let counter = counter.clone();
        let complete = complete.clone();

        handles.push(join_set.spawn(async move {
            provider.identify(tx).await;

            // Decrement counter and notify if we're the last task
            if counter.fetch_sub(1, Ordering::SeqCst) == 1 {
                complete.notify_one();
            }
        }));
    }

    tokio::select! {
        biased;

        // Priority 1: If we receive an identifier, return it immediately
        res = rx.recv() => {
            tracing::trace!("Received result from channel: {:?}", res);
            res.unwrap_or_default()
        }

        // Priority 2: If all tasks complete without finding an identifier
        _ = complete.notified() => {
            tracing::trace!("All providers have finished identifying");
            ProviderId::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_supported_providers() {
        let providers = supported_providers().await;
        assert_eq!(providers.len(), 9);
        assert!(providers.contains(&akamai::IDENTIFIER.to_string()));
        assert!(providers.contains(&alibaba::IDENTIFIER.to_string()));
        assert!(providers.contains(&aws::IDENTIFIER.to_string()));
        assert!(providers.contains(&azure::IDENTIFIER.to_string()));
        assert!(providers.contains(&digitalocean::IDENTIFIER.to_string()));
        assert!(providers.contains(&gcp::IDENTIFIER.to_string()));
        assert!(providers.contains(&oci::IDENTIFIER.to_string()));
        assert!(providers.contains(&openstack::IDENTIFIER.to_string()));
        assert!(providers.contains(&vultr::IDENTIFIER.to_string()));
    }
}
