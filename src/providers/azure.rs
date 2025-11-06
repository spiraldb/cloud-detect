//! Microsoft Azure.

use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::mpsc::Sender;

use crate::{Provider, ProviderId};

const METADATA_URI: &str = "http://169.254.169.254";
const METADATA_PATH: &str = "/metadata/instance?api-version=2017-12-01";
const VENDOR_FILE: &str = "/sys/class/dmi/id/sys_vendor";
pub(crate) const IDENTIFIER: ProviderId = ProviderId::Azure;

#[derive(Serialize, Deserialize)]
struct Compute {
    #[serde(rename = "vmId")]
    vm_id: String,
}

#[derive(Serialize, Deserialize)]
struct MetadataResponse {
    compute: Compute,
}

pub(crate) struct Azure;

#[async_trait]
impl Provider for Azure {
    fn identifier(&self) -> ProviderId {
        IDENTIFIER
    }

    /// Tries to identify Azure using all the implemented options.
    async fn identify(&self, tx: Sender<ProviderId>) {
        tracing::trace!("Checking Microsoft Azure");
        if self.check_vendor_file(VENDOR_FILE).await
            || self.check_metadata_server(METADATA_URI).await
        {
            tracing::trace!("Identified Microsoft Azure");
            let res = tx.send(IDENTIFIER).await;

            if let Err(err) = res {
                tracing::trace!("Error sending message: {:?}", err);
            }
        }
    }
}

impl Azure {
    /// Tries to identify Azure via metadata server.
    async fn check_metadata_server(&self, metadata_uri: &str) -> bool {
        let timeout = crate::DEFAULT_DETECTION_TIMEOUT;
        let url = format!("{metadata_uri}{METADATA_PATH}");
        tracing::trace!("Checking {} metadata using url: {}", IDENTIFIER, url);

        let client = if let Ok(client) = reqwest::Client::builder().timeout(timeout).build() {
            client
        } else {
            tracing::trace!("Error creating client");
            return false;
        };
        let req = client.get(url).header("Metadata", "true");

        match req.send().await {
            Ok(resp) => match resp.json::<MetadataResponse>().await {
                Ok(resp) => !resp.compute.vm_id.is_empty(),
                Err(err) => {
                    tracing::trace!("Error reading response: {:?}", err);
                    false
                }
            },
            Err(err) => {
                tracing::trace!("Error making request: {:?}", err);
                false
            }
        }
    }

    /// Tries to identify Azure using vendor file(s).
    async fn check_vendor_file<P: AsRef<Path>>(&self, vendor_file: P) -> bool {
        tracing::trace!(
            "Checking {} vendor file: {}",
            IDENTIFIER,
            vendor_file.as_ref().display()
        );

        if vendor_file.as_ref().is_file() {
            return match fs::read_to_string(vendor_file).await {
                Ok(content) => content.contains("Microsoft Corporation"),
                Err(err) => {
                    tracing::trace!("Error reading file: {:?}", err);
                    false
                }
            };
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use anyhow::Result;
    use tempfile::NamedTempFile;
    use wiremock::matchers::query_param;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn test_check_metadata_server_success() {
        let mock_server = MockServer::start().await;
        Mock::given(query_param("api-version", "2017-12-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(MetadataResponse {
                compute: Compute {
                    vm_id: "vm-123abc".to_string(),
                },
            }))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Azure;
        let metadata_uri = mock_server.uri();
        let result = provider.check_metadata_server(&metadata_uri).await;

        assert!(result);
    }

    #[tokio::test]
    async fn test_check_metadata_server_failure() {
        let mock_server = MockServer::start().await;
        Mock::given(query_param("api-version", "2017-12-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(MetadataResponse {
                compute: Compute {
                    vm_id: "".to_string(),
                },
            }))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Azure;
        let metadata_uri = mock_server.uri();
        let result = provider.check_metadata_server(&metadata_uri).await;

        assert!(!result);
    }

    #[tokio::test]
    async fn test_check_vendor_file_success() -> Result<()> {
        let mut vendor_file = NamedTempFile::new()?;
        vendor_file.write_all(b"Microsoft Corporation")?;

        let provider = Azure;
        let result = provider.check_vendor_file(vendor_file.path()).await;

        assert!(result);

        Ok(())
    }

    #[tokio::test]
    async fn test_check_vendor_file_failure() -> Result<()> {
        let vendor_file = NamedTempFile::new()?;

        let provider = Azure;
        let result = provider.check_vendor_file(vendor_file.path()).await;

        assert!(!result);

        Ok(())
    }
}
