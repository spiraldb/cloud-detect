//! Oracle Cloud Infrastructure (OCI).

use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::mpsc::Sender;

use crate::{Provider, ProviderId};

const METADATA_URI: &str = "http://169.254.169.254";
const METADATA_PATH: &str = "/opc/v1/instance/metadata/";
const VENDOR_FILE: &str = "/sys/class/dmi/id/chassis_asset_tag";
pub(crate) const IDENTIFIER: ProviderId = ProviderId::OCI;

#[derive(Serialize, Deserialize)]
struct MetadataResponse {
    #[serde(rename = "oke-tm")]
    oke_tm: String,
}

pub(crate) struct Oci;

#[async_trait]
impl Provider for Oci {
    fn identifier(&self) -> ProviderId {
        IDENTIFIER
    }

    /// Tries to identify OCI using all the implemented options.
    async fn identify(&self, tx: Sender<ProviderId>) {
        tracing::trace!("Checking Oracle Cloud Infrastructure");
        if self.check_vendor_file(VENDOR_FILE).await
            || self.check_metadata_server(METADATA_URI).await
        {
            tracing::trace!("Identified Oracle Cloud Infrastructure");
            let res = tx.send(IDENTIFIER).await;

            if let Err(err) = res {
                tracing::trace!("Error sending message: {:?}", err);
            }
        }
    }
}

impl Oci {
    /// Tries to identify OCI via metadata server.
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

        match client.get(url).send().await {
            Ok(resp) => match resp.json::<MetadataResponse>().await {
                Ok(resp) => resp.oke_tm.contains("oke"),
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

    /// Tries to identify OCI using vendor file(s).
    async fn check_vendor_file<P: AsRef<Path>>(&self, vendor_file: P) -> bool {
        tracing::trace!(
            "Checking {} vendor file: {}",
            IDENTIFIER,
            vendor_file.as_ref().display()
        );

        if vendor_file.as_ref().is_file() {
            return match fs::read_to_string(vendor_file).await {
                Ok(content) => content.contains("OracleCloud"),
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
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn test_check_metadata_server_success() {
        let mock_server = MockServer::start().await;
        Mock::given(path(METADATA_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(MetadataResponse {
                oke_tm: "oke".to_string(),
            }))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Oci;
        let metadata_uri = mock_server.uri();
        let result = provider.check_metadata_server(&metadata_uri).await;

        assert!(result);
    }

    #[tokio::test]
    async fn test_check_metadata_server_failure() {
        let mock_server = MockServer::start().await;
        Mock::given(path(METADATA_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(MetadataResponse {
                oke_tm: "abc".to_string(),
            }))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Oci;
        let metadata_uri = mock_server.uri();
        let result = provider.check_metadata_server(&metadata_uri).await;

        assert!(!result);
    }

    #[tokio::test]
    async fn test_check_vendor_file_success() -> Result<()> {
        let mut vendor_file = NamedTempFile::new()?;
        vendor_file.write_all(b"OracleCloud")?;

        let provider = Oci;
        let result = provider.check_vendor_file(vendor_file.path()).await;

        assert!(result);

        Ok(())
    }

    #[tokio::test]
    async fn test_check_vendor_file_failure() -> Result<()> {
        let vendor_file = NamedTempFile::new()?;

        let provider = Oci;
        let result = provider.check_vendor_file(vendor_file.path()).await;

        assert!(!result);

        Ok(())
    }
}
