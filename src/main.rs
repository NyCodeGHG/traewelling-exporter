use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::Duration,
};

use axum::{extract::State, routing::get, Router};
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
use reqwest::Url;
use traewelling_exporter::traewelling::client::TraewellingClient;

fn init_meter() -> PrometheusExporter {
    let controller = controllers::basic(
        processors::factory(
            selectors::simple::histogram([1.0, 2.0, 5.0, 10.0, 20.0, 50.0]),
            aggregation::cumulative_temporality_selector(),
        )
        .with_memory(true),
    )
    .with_resource(Resource::default())
    .build();

    opentelemetry_prometheus::exporter(controller).init()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let _ = dotenvy::dotenv();

    let exporter = init_meter();

    let metrics = create_metrics();

    let client = TraewellingClient::builder()
        .with_base_url(Url::parse("http://localhost:8000/api/v1").unwrap())
        .with_token(std::env::var("TRAEWELLING_TOKEN").ok())
        .build();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        let cx = Context::current();
        loop {
            interval.tick().await;
            let checkins = match client.statuses().get_active_statuses().await {
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
                    continue;
                }
            };
            tracing::trace!("Observing {} checkins", checkins.len());
            for (_, data) in &checkins
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
            {
                let data = data.collect_vec();
                let length = data.len();
                let first = data.into_iter().next().unwrap();
                let key_value: Vec<KeyValue> = first.into();
                metrics.checkins.observe(&cx, length as u64, &key_value);
            }
        }
    });

    let app_state = AppState { exporter };

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(app_state);

    let address = "0.0.0.0:3000".parse().unwrap();
    let server = axum::Server::bind(&address).serve(app.into_make_service());
    tracing::info!("Server listening on http://{}", address);
    server.await.unwrap();
}

#[derive(Hash, Debug)]
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

async fn metrics_handler<'a>(State(AppState { exporter, .. }): State<AppState>) -> String {
    let encoder = TextEncoder::new();
    let metrics = exporter.registry().gather();
    encoder.encode_to_string(&metrics).unwrap()
}
