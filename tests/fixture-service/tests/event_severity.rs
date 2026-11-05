//! An event's `severity:` reaches its log record, and an event without one
//! has none.

mod common;

use fixture_semconv::event;
use opentelemetry::logs::Severity;

#[test]
fn an_events_severity_is_set_on_its_log_record() {
    let t = common::telemetry();
    semlex::init(t.config()).unwrap();

    event!(session.start { session.id: "s-1" }, severity: warn);
    event!(session.start { session.id: "s-2" });

    let severities: Vec<_> = t
        .logs
        .get_emitted_logs()
        .unwrap()
        .iter()
        .map(|log| (log.record.severity_number(), log.record.severity_text()))
        .collect();
    assert_eq!(
        severities,
        [(Some(Severity::Warn), Some("WARN")), (None, None)]
    );
}
