#![feature(const_slice_index)]

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
};

use axum::{extract::State, response::Redirect, routing::get, Router};
use cached::proc_macro::cached;
use itertools::Itertools;
use prometheus::{
    opts, register_int_counter, register_int_gauge_vec, IntCounter, IntGaugeVec, Registry, TextEncoder,
};
use reqwest::StatusCode;
use traewelling_exporter::traewelling::client::TraewellingClient;

lazy_static::lazy_static! {
    static ref CLIENT: TraewellingClient = TraewellingClient::builder()
        .with_base_url(
            std::env::var("TRAEWELLING_API")
                .ok()
                .and_then(|var| var.parse().ok())
                .unwrap_or_else(|| "https://traewelling.de/api/v1".parse().unwrap()),
        )
        .with_token(std::env::var("TRAEWELLING_TOKEN").ok())
        .build();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let _ = dotenvy::dotenv();

    let metrics = create_metrics()?;

    let app_state = AppState { metrics };

    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/metrics") }))
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(|| async { StatusCode::OK }))
        .with_state(app_state);

    let address = "0.0.0.0:3000".parse()?;
    let server = axum::Server::bind(&address)
        .serve(app.into_make_service())
        .with_graceful_shutdown(shutdown_signal());
    tracing::info!("Server listening on http://localhost:3000");
    server.await?;
    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to register signal hook")
}

#[derive(Hash, Debug, PartialEq, Eq, Clone)]
struct CheckinData {
    pub category: String,
    pub distance: String,
    pub line_name: String,
    pub number: String,
    pub duration: String,
    pub speed: String,
}

impl<'a> From<&'a CheckinData> for HashMap<&str, &'a str> {
    fn from(data: &'a CheckinData) -> Self {
        HashMap::from([
            ("checkin_category", data.category.as_str()),
            ("checkin_distance", data.distance.as_str()),
            ("checkin_line_name", data.line_name.as_str()),
            ("checkin_number", data.number.as_str()),
            ("checkin_duration", data.duration.as_str()),
            ("checkin_speed", data.speed.as_str()),
        ])
    }
}

#[derive(Clone)]
struct AppState {
    metrics: Metrics,
}

#[derive(Clone)]
struct Metrics {
    checkins: IntGaugeVec,
    traewelling_requests: IntCounter,
}

fn create_metrics() -> Result<Metrics, prometheus::Error> {
    let checkins = register_int_gauge_vec!(
        "journeys",
        "Current Journeys",
        &[
            "checkin_category",
            "checkin_distance",
            "checkin_line_name",
            "checkin_number",
            "checkin_duration",
            "checkin_speed"
        ]
    )?;
    let traewelling_requests = register_int_counter!(opts!(
        "traewelling_requests",
        "HTTP Requests sent to Traewelling API"
    ))?;
    Ok(Metrics {
        checkins,
        traewelling_requests,
    })
}

async fn metrics_handler<'a>(
    State(AppState { metrics }): State<AppState>,
) -> Result<String, String> {
    let Ok(data) = fetch_metrics(&metrics, "metrics").await else {
        return Err("Failed to fetch journeys".to_string());
    };
    record_metrics(data, &metrics);

    let mut text = String::new();
    let encoder = TextEncoder::new();
    let metrics = Registry::default().gather();
    text += &encoder.encode_to_string(&metrics).unwrap();
    text += "\n\n";
    let metrics = prometheus::gather();
    text += &encoder.encode_to_string(&metrics).unwrap();
    Ok(text)
}

#[cached(
    time = 2,
    sync_writes = true,
    key = "String",
    result = true,
    convert = r#"{String::from(_cache_key)}"#
)]
async fn fetch_metrics(
    metrics: &Metrics,
    _cache_key: &str,
) -> Result<Vec<(CheckinData, usize)>, ()> {
    let checkins = match CLIENT.statuses().get_active_statuses().await {
        Ok(data) => {
            metrics.traewelling_requests.inc();
            data.data
        }
        Err(e) => {
            metrics.traewelling_requests.inc();
            tracing::error!("Traewelling Request failed: {}", e);
            return Err(());
        }
    };
    tracing::trace!("Observing {} checkins", checkins.len());
    let checkins = checkins
        .into_iter()
        .map(|checkin| CheckinData {
            category: checkin.train.category,
            line_name: checkin.train.line_name,
            distance: checkin.train.distance.to_string(),
            duration: checkin.train.duration.to_string(),
            number: checkin.train.number,
            speed: checkin.train.speed.to_string(),
        })
        .group_by(|data| {
            let mut hasher = DefaultHasher::new();
            data.hash(&mut hasher);
            hasher.finish()
        })
        .into_iter()
        .map(|(_, group)| {
            let data: Vec<CheckinData> = group.collect();
            let length = data.len();
            let first = data.into_iter().next().unwrap();
            (first, length)
        })
        .collect();
    Ok(checkins)
}

fn record_metrics(data: Vec<(CheckinData, usize)>, metrics: &Metrics) {
    metrics.checkins.reset();
    for (ref checkin, amount) in data {
        let map = checkin.into();
        metrics
            .checkins
            .get_metric_with(&map)
            .unwrap()
            .set(amount as i64);
    }
}
