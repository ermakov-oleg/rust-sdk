use std::sync::Mutex;

use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use tracing::{error, info, span, Level};
use tracing_bunyan_formatter::JsonStorageLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

use lazy_static::lazy_static;
use struct_log::JsonLogLayer;

use crate::mock_writer::MockWriter;

mod mock_writer;

/// Tests have to be run on a single thread because we are re-using the same buffer for
/// all of them.
type InMemoryBuffer = Mutex<Vec<u8>>;
lazy_static! {
    static ref BUFFER: InMemoryBuffer = Mutex::new(vec![]);
}

// Run a closure and collect the output emitted by the tracing instrumentation using an in-memory buffer.
fn run_and_get_raw_output<F: Fn()>(action: F) -> String {
    let formatting_layer = JsonLogLayer::new("test-app".into(), "e2e".to_string(), || {
        MockWriter::new(&BUFFER)
    });
    let subscriber = Registry::default()
        .with(JsonStorageLayer)
        .with(formatting_layer);
    tracing::subscriber::with_default(subscriber, action);

    // Return the formatted output as a string to make assertions against
    let mut buffer = BUFFER.lock().unwrap();
    let output = buffer.to_vec();
    // Clean the buffer to avoid cross-tests interactions
    buffer.clear();
    String::from_utf8(output).unwrap()
}

// Run a closure and collect the output emitted by the tracing instrumentation using
// an in-memory buffer as structured new-line-delimited JSON.
fn run_and_get_output<F: Fn()>(action: F) -> Vec<Value> {
    run_and_get_raw_output(action)
        .lines()
        .filter(|&l| !l.is_empty())
        .inspect(|l| println!("{}", l))
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}

// Instrumented code to be run to test the behaviour of the tracing instrumentation.
fn test_action() {
    info!("foo");
    error!(foo = "baz", "bar");
}

#[test]
fn each_line_is_valid_json() {
    let tracing_output = run_and_get_raw_output(test_action);

    // Each line is valid JSON
    for line in tracing_output.lines().filter(|&l| !l.is_empty()) {
        assert!(serde_json::from_str::<Value>(line).is_ok());
    }
}

#[test]
fn each_line_has_the_base_fields() {
    let tracing_output = run_and_get_output(test_action);

    for record in tracing_output {
        println!("{}", record);
        assert!(record.get("runtime").is_some());
        assert!(record.get("level").is_some());
        assert!(record.get("date").is_some());
        assert!(record.get("message").is_some());
        assert!(record.get("file").is_some());
        assert!(record.get("lineno").is_some());
        assert!(record.get("logger").is_some());
        assert!(record.get("container_id").is_some());
        assert!(record.get("version").is_some());
        assert!(record.get("application").is_some());
    }
}

#[test]
fn time_is_formatted_according_to_rfc_3339() {
    let tracing_output = run_and_get_output(test_action);

    for record in tracing_output {
        let time = record.get("date").unwrap().as_str().unwrap();
        let parsed = time::OffsetDateTime::parse(time, &Rfc3339);
        assert!(parsed.is_ok());
        let parsed = parsed.unwrap();
        assert!(parsed.offset().is_utc());
    }
}
