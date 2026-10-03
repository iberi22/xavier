//! Mini-expert training jobs (MX-04): SQLite job table, `ComputeBackend`
//! abstraction (Local + ManualNotebook only) and the `/v1/training/jobs` API.
//!
//! Layout under the training data dir:
//! `training_jobs.sqlite3`, `jobs/<id>/{logs.txt,out/,artifacts/}`.

pub mod backend;
pub mod jobs;
pub mod routes;
