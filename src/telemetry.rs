use opentelemetry_sdk::Resource;
use tracing_subscriber::{Layer, Registry, layer::SubscriberExt};

pub fn init_telemetry() {
    let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to create span exporter");

    let metrics_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to create metric exporter");

    let resource = Resource::builder_empty()
        .with_attributes([opentelemetry::KeyValue::new(
            "service.name",
            crate::config::SERVICE_NAME,
        )])
        .build();

    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(otlp_exporter)
        .with_resource(resource.clone())
        .build();

    let meter_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
        .with_reader(opentelemetry_sdk::metrics::PeriodicReader::builder(metrics_exporter).build())
        .with_resource(resource)
        .build();

    opentelemetry::global::set_tracer_provider(tracer_provider);
    opentelemetry::global::set_meter_provider(meter_provider);

    let tracer = opentelemetry::global::tracer(crate::config::SERVICE_NAME);

    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    let subscriber = Registry::default().with(telemetry).with(
        tracing_subscriber::fmt::layer()
            .pretty()
            .with_filter(tracing_subscriber::filter::LevelFilter::INFO),
    );

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}
