use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use axum::{extract::State, response::Html, routing::get, Router};
use cached::proc_macro::cached;
use itertools::Itertools;
use opentelemetry::{
    global,
    metrics::{Counter, ObservableGauge},
    sdk::{
        export::metrics::aggregation,
        metrics::{controllers, processors, selectors},
        Resource,
    },
    Context, KeyValue,
};
use opentelemetry_prometheus::{PrometheusExporter, TextEncoder};
use reqwest::StatusCode;
use traewelling_exporter::traewelling::client::TraewellingClient;

fn init_meter() -> PrometheusExporter {
    let controller = controllers::basic(processors::factory(
        selectors::simple::histogram([1.0, 2.0, 5.0, 10.0, 20.0, 50.0]),
        aggregation::cumulative_temporality_selector(),
    ))
    .with_resource(Resource::default())
    .build();

    opentelemetry_prometheus::exporter(controller).init()
}

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
async fn main() {
    tracing_subscriber::fmt::init();
    let _ = dotenvy::dotenv();

    let exporter = init_meter();

    let metrics = create_metrics();

    let app_state = AppState { exporter, metrics };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/metrics", get(metrics_handler))
        .route("/healthz", get(|| async { StatusCode::OK }))
        .with_state(app_state);

    let address = "0.0.0.0:3000".parse().unwrap();
    let server = axum::Server::bind(&address).serve(app.into_make_service());
    tracing::info!("Server listening on http://{}", address);
    server.await.unwrap();
}

#[derive(Hash, Debug, PartialEq, Eq, Clone)]
struct CheckinData {
    pub category: String,
    pub distance: i32,
    pub line_name: String,
    pub number: String,
    pub duration: i32,
    pub speed: String,
}

impl From<CheckinData> for Vec<KeyValue> {
    fn from(data: CheckinData) -> Self {
        vec![
            KeyValue::new("checkin.category", data.category),
            KeyValue::new("checkin.distance", data.distance.to_string()),
            KeyValue::new("checkin.line_name", data.line_name),
            KeyValue::new("checkin.number", data.number),
            KeyValue::new("checkin.duration", data.duration.to_string()),
            KeyValue::new("checkin.speed", data.speed),
        ]
    }
}

#[derive(Clone)]
struct AppState {
    exporter: PrometheusExporter,
    metrics: Metrics,
}

#[derive(Clone)]
struct Metrics {
    checkins: ObservableGauge<u64>,
    traewelling_requests: Counter<u64>,
}

fn create_metrics() -> Metrics {
    let meter = global::meter("traewelling-exporter");
    let checkins = meter
        .u64_observable_gauge("journeys")
        .with_description("Current Journeys")
        .init();
    let traewelling_requests = meter
        .u64_counter("traewelling_requests")
        .with_description("HTTP Requests sent to Traewelling API")
        .init();
    Metrics {
        checkins,
        traewelling_requests,
    }
}

async fn metrics_handler<'a>(
    State(AppState { exporter, metrics }): State<AppState>,
) -> Result<String, String> {
    let Ok(data) = fetch_metrics(&metrics, "metrics").await else {
        return Err("Failed to fetch journeys".to_string());
    };
    record_metrics(data, &metrics);

    let mut text = String::new();
    let encoder = TextEncoder::new();
    let metrics = exporter.registry().gather();
    text += &encoder.encode_to_string(&metrics).unwrap();
    text += "\n\n";
    let metrics = prometheus::gather();
    text += &encoder.encode_to_string(&metrics).unwrap();
    Ok(text)
}

#[cached(
    time = 30,
    sync_writes = true,
    key = "String",
    result = true,
    convert = r#"{String::from(_cache_key)}"#
)]
async fn fetch_metrics(
    metrics: &Metrics,
    _cache_key: &str,
) -> Result<Vec<(CheckinData, usize)>, ()> {
    let cx = Context::current();
    let checkins = match CLIENT.statuses().get_active_statuses().await {
        Ok(data) => {
            metrics
                .traewelling_requests
                .add(&cx, 1, &[KeyValue::new("success", true)]);
            data.data
        }
        Err(e) => {
            metrics
                .traewelling_requests
                .add(&cx, 1, &[KeyValue::new("success", false)]);
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
            distance: checkin.train.distance,
            duration: checkin.train.duration,
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
    let cx = Context::current();
    for (checkin, amount) in data {
        let key_value: Vec<KeyValue> = checkin.into();
        metrics.checkins.observe(&cx, amount as u64, &key_value);
    }
}

async fn index_handler() -> Html<String> {
    Html(include_str!("index.html").replace("%VERSION%", env!("CARGO_PKG_VERSION")))
}
