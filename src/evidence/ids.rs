use uuid::Uuid;

/// Generate a Tracebox trace ID.
///
/// UUIDv7 is used instead of timestamp-only IDs because it gives:
///
/// - natural sortability by creation time;
/// - extremely low collision risk;
/// - compatibility with distributed future ingestion;
/// - no dependence on a centralized sequence allocator.
///
/// The `trc_` prefix deliberately makes the ID self-describing in logs,
/// manifests, and future UI surfaces.
pub fn generate_trace_id() -> String {
    format!("trc_{}", Uuid::now_v7())
}
