//! Alibaba Cloud.

use std::path::Path;

use async_trait::async_trait;
use tokio::fs;
use tokio::sync::mpsc::Sender;

use crate::{Provider, ProviderId, DEFAULT_DETECTION_TIMEOUT};

const METADATA_URI: &str = "http://100.100.100.200";
const METADATA_PATH: &str = "/latest/meta-data/latest/meta-data/instance/virtualization-solution";
const VENDOR_FILE: &str = "/sys/class/dmi/id/product_name";
pub(crate) const IDENTIFIER: ProviderId = ProviderId::Alibaba;

pub(crate) struct Alibaba;

#[async_trait]
impl Provider for Alibaba {
    fn identifier(&self) -> ProviderId {
        IDENTIFIER
    }

    /// Tries to identify Alibaba Cloud using all the implemented options.
    async fn identify(&self, tx: Sender<ProviderId>) {
        tracing::trace!("Checking Alibaba Cloud");
        if self.check_vendor_file(VENDOR_FILE).await
            || self.check_metadata_server(METADATA_URI).await
        {
            tracing::trace!("Identified Alibaba Cloud");
            let res = tx.send(IDENTIFIER).await;

            if let Err(err) = res {
                tracing::trace!("Error sending message: {:?}", err);
            }
        }
    }
}

impl Alibaba {
    /// Tries to identify Alibaba via metadata server.
    async fn check_metadata_server(&self, metadata_uri: &str) -> bool {
        let url = format!("{metadata_uri}{METADATA_PATH}");
        tracing::trace!("Checking {} metadata using url: {}", IDENTIFIER, url);

        let client = if let Ok(client) = reqwest::Client::builder()
            .timeout(DEFAULT_DETECTION_TIMEOUT)
            .build()
        {
            client
        } else {
            tracing::trace!("Error creating client");
            return false;
        };

        match client.get(url).send().await {
            Ok(resp) => match resp.text().await {
                Ok(text) => text.contains("ECS Virt"),
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

    /// Tries to identify Alibaba using vendor file(s).
    async fn check_vendor_file<P: AsRef<Path>>(&self, vendor_file: P) -> bool {
        tracing::trace!(
            "Checking {} vendor file: {}",
            IDENTIFIER,
            vendor_file.as_ref().display()
        );

        if vendor_file.as_ref().is_file() {
            return match fs::read_to_string(vendor_file).await {
                Ok(content) => content.contains("Alibaba Cloud ECS"),
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
            .respond_with(ResponseTemplate::new(200).set_body_string("ECS Virt"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Alibaba;
        let metadata_uri = mock_server.uri();
        let result = provider.check_metadata_server(&metadata_uri).await;

        assert!(result);
    }

    #[tokio::test]
    async fn test_check_metadata_server_failure() {
        let mock_server = MockServer::start().await;
        Mock::given(path(METADATA_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string("ABC"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Alibaba;
        let metadata_uri = mock_server.uri();
        let result = provider.check_metadata_server(&metadata_uri).await;

        assert!(!result);
    }

    #[tokio::test]
    async fn test_check_vendor_file_success() -> Result<()> {
        let mut vendor_file = NamedTempFile::new()?;
        vendor_file.write_all(b"Alibaba Cloud ECS")?;

        let provider = Alibaba;
        let result = provider.check_vendor_file(vendor_file.path()).await;

        assert!(result);

        Ok(())
    }

    #[tokio::test]
    async fn test_check_vendor_file_failure() -> Result<()> {
        let vendor_file = NamedTempFile::new()?;

        let provider = Alibaba;
        let result = provider.check_vendor_file(vendor_file.path()).await;

        assert!(!result);

        Ok(())
    }
}
