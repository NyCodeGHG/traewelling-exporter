use reqwest::StatusCode;
use thiserror::Error;

pub mod traewelling {
    pub mod client {
        use std::env;

        use chrono::{DateTime, FixedOffset};
        pub use reqwest::Client;
        use reqwest::Url;
        use serde::{Deserialize, Serialize};

        use crate::Error;

        #[derive(Clone)]
        pub struct TraewellingClient {
            base_url: Url,
            client: Client,
            token: Option<String>,
        }

        const DEFAULT_TRAEWELLING_BASE_URL: &str = "https://traewelling.de/api/v1";

        impl Default for TraewellingClient {
            fn default() -> Self {
                Self {
                    base_url: Url::parse(DEFAULT_TRAEWELLING_BASE_URL).unwrap(),
                    client: create_default_client(),
                    token: None,
                }
            }
        }

        fn create_default_client() -> Client {
            Client::builder()
                .user_agent(concat!(
                    env!("CARGO_PKG_NAME"),
                    "/",
                    env!("CARGO_PKG_VERSION")
                ))
                .build()
                .expect("Failed to create reqwest client")
        }

        #[derive(Default)]
        pub struct TraewellingClientBuilder {
            base_url: Option<Url>,
            client: Option<Client>,
            token: Option<String>,
        }

        impl TraewellingClientBuilder {
            pub fn with_base_url(mut self, base_url: Url) -> Self {
                self.base_url = Some(base_url);
                self
            }

            pub fn with_client(mut self, client: Client) -> Self {
                self.client = Some(client);
                self
            }

            pub fn with_token<T: Into<Option<String>>>(mut self, token: T) -> Self {
                self.token = token.into();
                self
            }

            pub fn build(self) -> TraewellingClient {
                TraewellingClient {
                    base_url: self
                        .base_url
                        .unwrap_or_else(|| Url::parse(DEFAULT_TRAEWELLING_BASE_URL).unwrap()),
                    client: create_default_client(),
                    token: self.token,
                }
            }
        }

        impl TraewellingClient {
            pub fn builder() -> TraewellingClientBuilder {
                TraewellingClientBuilder::default()
            }
            pub fn statuses(&self) -> StatusCategory {
                StatusCategory { client: self }
            }
        }

        pub struct StatusCategory<'a> {
            client: &'a TraewellingClient,
        }

        impl<'a> StatusCategory<'a> {
            pub async fn get_active_statuses(&self) -> Result<ActiveStatusesResponse, Error> {
                let mut request = self
                    .client
                    .client
                    .get(format!("{}/statuses", self.client.base_url));
                if let Some(token) = self.client.token.as_ref() {
                    request = request.bearer_auth(token.as_str());
                }
                let response = request.send().await?;
                if !response.status().is_success() {
                    return Err(Error::InvalidTrwlResponse(crate::TrwlErrorResponse {
                        status_code: response.status(),
                        message: response.text().await?,
                    }));
                }
                Ok(response.json().await?)
            }
        }

        #[derive(Debug, Deserialize, Serialize)]
        pub struct ActiveStatusesResponse {
            pub data: Vec<Status>,
        }
        #[derive(Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Status {
            pub id: i32,
            pub user: i32,
            pub username: String,
            pub business: i32,
            pub created_at: DateTime<FixedOffset>,
            pub train: Train,
            pub event: Option<Event>,
        }

        #[derive(Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Train {
            pub trip: i32,
            pub hafas_id: String,
            pub category: String,
            pub number: String,
            pub line_name: String,
            pub distance: i32,
            pub points: i32,
            pub duration: i32,
            pub speed: f64,
            pub origin: TrainStopover,
            pub destination: TrainStopover,
        }

        #[derive(Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Event {
            pub id: i32,
            pub name: String,
        }

        #[derive(Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct TrainStopover {
            pub id: i32,
            pub name: String,
            pub eva_identifier: i32,
            pub arrival: Option<DateTime<FixedOffset>>,
            pub arrival_planned: Option<DateTime<FixedOffset>>,
            pub arrival_real: Option<DateTime<FixedOffset>>,
            pub arrival_platform_planned: Option<String>,
            pub arrival_platform_real: Option<String>,
            pub departure: Option<DateTime<FixedOffset>>,
            pub departure_planned: Option<DateTime<FixedOffset>>,
            pub departure_real: Option<DateTime<FixedOffset>>,
            pub departure_platform_planned: Option<String>,
            pub platform: Option<String>,
            pub is_arrival_delayed: bool,
            pub is_departure_delayed: bool,
            pub cancelled: bool,
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error("Got an invalid response from traewelling: {0:#?}")]
    InvalidTrwlResponse(TrwlErrorResponse),
}

#[derive(Debug)]
#[allow(dead_code)] // Debug message
pub struct TrwlErrorResponse {
    status_code: StatusCode,
    message: String,
}
