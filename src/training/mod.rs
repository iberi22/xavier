//! Mini-expert training jobs (MX-04): SQLite job table, `ComputeBackend`
//! abstraction (Local + ManualNotebook only) and the `/v1/training/jobs` API.
//!
//! Layout under the training data dir:
//! `training_jobs.sqlite3`, `jobs/<id>/{logs.txt,out/,artifacts/,notebook/}`.
//! Terminal jobs' heavy directories are pruned by `retention`.

pub mod backend;
pub mod jobs;
pub mod retention;
pub mod routes;
