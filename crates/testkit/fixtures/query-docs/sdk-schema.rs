#![forbid(unsafe_code)]

use unisphere_sdk::query::{schema, Dataset, FieldId, OperationKind, OutputFormat};

fn main() {
    // Static schema discovery acquires no source, config, Git, environment, or network capability.
    let tools = schema(Dataset::Tools);
    assert_eq!(tools.schema_version, 1);
    assert!(tools.field(FieldId::DurationMs).is_some());
    assert!(tools.permitted_operations.contains(&OperationKind::Stats));
    for metric_field in [
        FieldId::Count,
        FieldId::MeasuredCount,
        FieldId::MissingDurationCount,
        FieldId::Failures,
        FieldId::FailureRate,
        FieldId::P95Ms,
    ] {
        assert!(
            tools.field(metric_field).is_some(),
            "stats result field must be discoverable: {}",
            metric_field.as_str()
        );
    }
    assert!(
        tools
            .format(OperationKind::Stats, OutputFormat::Json)
            .is_some()
    );

    println!(
        "{}",
        serde_json::to_string(tools).expect("query schema is serializable")
    );
}
