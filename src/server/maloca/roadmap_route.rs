//! Read-only roadmap endpoint (`GET /v1/maloca/roadmap`).
//!
//! Path: src/server/maloca/roadmap_route.rs
//!
//! Groups the features scanned from the GitCore ledgers by wave (ascending).
//! Features without a wave are reported under `unscheduled`. Per group it
//! returns status counts and the mean `progress_pct`.

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::backlog_route::{UnifiedBacklogService, UnifiedProject};

/// One feature as shown on the roadmap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoadmapFeature {
    pub id: String,
    pub name: String,
    pub app_id: String,
    pub status: String,
    pub progress_pct: f64,
}

/// Aggregated counts and progress for a set of features.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoadmapSummary {
    pub total: usize,
    pub stable: usize,
    pub in_progress: usize,
    pub planned: usize,
    pub progress_pct: f64,
    pub features: Vec<RoadmapFeature>,
}

/// One wave of the roadmap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoadmapWave {
    pub wave: u32,
    #[serde(flatten)]
    pub summary: RoadmapSummary,
}

/// Response body for `GET /v1/maloca/roadmap`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoadmapResponse {
    pub waves: Vec<RoadmapWave>,
    pub unscheduled: RoadmapSummary,
}

/// Statuses counted as done on the roadmap.
fn is_done(status: &str) -> bool {
    matches!(status, "stable" | "completed")
}

impl RoadmapSummary {
    /// Builds the summary from a list of features. Features are sorted by id
    /// so the output does not depend on directory scan order.
    ///
    /// Every feature lands in exactly one bucket: `stable` (stable or
    /// completed), `planned`, or `in_progress` (any other status).
    fn from_features(mut features: Vec<RoadmapFeature>) -> Self {
        features.sort_by(|a, b| a.id.cmp(&b.id));
        let total = features.len();
        let stable = features.iter().filter(|f| is_done(&f.status)).count();
        let planned = features.iter().filter(|f| f.status == "planned").count();
        let in_progress = features
            .iter()
            .filter(|f| !is_done(&f.status) && f.status != "planned")
            .count();
        let progress_pct = if total == 0 {
            0.0
        } else {
            features.iter().map(|f| f.progress_pct).sum::<f64>() / total as f64
        };
        Self {
            total,
            stable,
            in_progress,
            planned,
            progress_pct,
            features,
        }
    }
}

/// Groups the features of all projects by wave.
pub fn build_roadmap(projects: &[UnifiedProject]) -> RoadmapResponse {
    let mut by_wave: BTreeMap<u32, Vec<RoadmapFeature>> = BTreeMap::new();
    let mut unscheduled = Vec::new();

    for item in projects.iter().flat_map(|p| p.features.iter()) {
        let feature = RoadmapFeature {
            id: item.id.clone(),
            name: item.name.clone(),
            app_id: item.app_id.clone(),
            status: item.status.clone(),
            progress_pct: item.progress_pct,
        };
        match item.wave {
            Some(wave) => by_wave.entry(wave).or_default().push(feature),
            None => unscheduled.push(feature),
        }
    }

    RoadmapResponse {
        waves: by_wave
            .into_iter()
            .map(|(wave, features)| RoadmapWave {
                wave,
                summary: RoadmapSummary::from_features(features),
            })
            .collect(),
        unscheduled: RoadmapSummary::from_features(unscheduled),
    }
}

/// GET `/v1/maloca/roadmap`: Read-only roadmap grouped by wave.
pub async fn roadmap_handler(State(service): State<UnifiedBacklogService>) -> impl IntoResponse {
    let projects = service.get_projects();
    Json(build_roadmap(&projects))
}

/// Router for the roadmap endpoint.
pub fn router(service: UnifiedBacklogService) -> Router {
    Router::new()
        .route("/v1/maloca/roadmap", get(roadmap_handler))
        .with_state(service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::maloca::backlog_route::UnifiedFeatureItem;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use std::fs::{create_dir_all, write};
    use tempfile::tempdir;
    use tower::util::ServiceExt;

    fn item(id: &str, wave: Option<u32>, status: &str, progress_pct: f64) -> UnifiedFeatureItem {
        UnifiedFeatureItem {
            id: id.to_string(),
            name: format!("Feature {id}"),
            app_id: "xavier".to_string(),
            wave,
            status: status.to_string(),
            priority: None,
            progress_pct,
            category: None,
            notes: None,
            github_issue: None,
        }
    }

    fn project(features: Vec<UnifiedFeatureItem>) -> UnifiedProject {
        UnifiedProject {
            repo_name: "xavier".to_string(),
            features,
        }
    }

    #[test]
    fn waves_are_ascending_and_unscheduled_is_separate() {
        let projects = vec![project(vec![
            item("c", Some(3), "planned", 0.0),
            item("a", None, "planned", 0.0),
            item("b", Some(1), "stable", 100.0),
            item("d", Some(1), "planned", 0.0),
        ])];

        let roadmap = build_roadmap(&projects);

        let waves: Vec<u32> = roadmap.waves.iter().map(|w| w.wave).collect();
        assert_eq!(waves, vec![1, 3]);
        assert_eq!(roadmap.waves[0].summary.total, 2);
        assert_eq!(roadmap.waves[1].summary.total, 1);
        assert_eq!(roadmap.unscheduled.total, 1);
        assert_eq!(roadmap.unscheduled.features[0].id, "a");
        assert!(roadmap
            .waves
            .iter()
            .all(|w| w.summary.features.iter().all(|f| f.id != "a")));
    }

    #[test]
    fn counts_and_progress_mean_per_group() {
        let projects = vec![
            project(vec![
                item("s", Some(2), "stable", 100.0),
                item("p", Some(2), "planned", 0.0),
            ]),
            project(vec![item("i", Some(2), "in_progress", 50.0)]),
        ];

        let roadmap = build_roadmap(&projects);
        let wave = &roadmap.waves[0];

        assert_eq!(wave.wave, 2);
        assert_eq!(wave.summary.total, 3);
        assert_eq!(wave.summary.stable, 1);
        assert_eq!(wave.summary.in_progress, 1);
        assert_eq!(wave.summary.planned, 1);
        assert!((wave.summary.progress_pct - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn every_status_lands_in_exactly_one_bucket() {
        let statuses = [
            "stable",
            "completed",
            "planned",
            "in_progress",
            "implemented",
            "partial",
            "beta",
            "blocked",
        ];
        let features = statuses
            .iter()
            .enumerate()
            .map(|(i, s)| item(&format!("f{i}"), Some(1), s, 0.0))
            .collect();
        let roadmap = build_roadmap(&[project(features)]);
        let summary = &roadmap.waves[0].summary;

        assert_eq!(summary.stable, 2, "stable and completed count as done");
        assert_eq!(summary.planned, 1);
        assert_eq!(
            summary.in_progress, 5,
            "implemented, partial, beta and blocked fall into in_progress"
        );
        assert_eq!(summary.total, statuses.len());
        assert_eq!(
            summary.total,
            summary.stable + summary.in_progress + summary.planned,
            "every feature must be counted once"
        );
    }

    #[test]
    fn empty_input_yields_empty_roadmap() {
        let roadmap = build_roadmap(&[]);

        assert!(roadmap.waves.is_empty());
        assert_eq!(roadmap.unscheduled.total, 0);
        assert_eq!(roadmap.unscheduled.progress_pct, 0.0);
    }

    #[tokio::test]
    async fn roadmap_endpoint_reads_gitcore_ledgers() {
        let tmp = tempdir().unwrap();
        let gitcore = tmp.path().join("shelf").join(".gitcore");
        create_dir_all(&gitcore).unwrap();
        write(
            gitcore.join("features.json"),
            r#"{
                "protocol": "3.8.0",
                "metadata": { "project": "shelf" },
                "features": [
                    { "id": "feat-w2", "name": "Wave 2", "status": "planned", "progress_pct": 0.0, "wave": 2 },
                    { "id": "feat-w1", "name": "Wave 1", "status": "stable", "progress_pct": 100.0, "wave": 1 },
                    { "id": "feat-none", "name": "Loose", "status": "planned", "progress_pct": 10.0 }
                ]
            }"#,
        )
        .unwrap();

        let service = UnifiedBacklogService::new().with_workspace_dir(tmp.path().to_path_buf());
        let app = router(service);
        let req = Request::builder()
            .method("GET")
            .uri("/v1/maloca/roadmap")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let roadmap: RoadmapResponse = serde_json::from_slice(&body).unwrap();

        let waves: Vec<u32> = roadmap.waves.iter().map(|w| w.wave).collect();
        assert_eq!(waves, vec![1, 2]);
        assert_eq!(roadmap.waves[0].summary.stable, 1);
        assert_eq!(roadmap.waves[1].summary.planned, 1);
        assert_eq!(roadmap.unscheduled.total, 1);
        assert_eq!(roadmap.unscheduled.features[0].id, "feat-none");
    }
}
