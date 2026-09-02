#![forbid(unsafe_code)]

//! CPAL audio I/O adapter boundary.
//!
//! Phase 0 validates device discovery in the `list_devices` example. Stream construction and
//! format negotiation are intentionally deferred until the headless-monitor phases.
