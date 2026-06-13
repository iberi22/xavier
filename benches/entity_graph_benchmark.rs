use criterion::{criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;
use xavier::memory::entity_graph::EntityGraph;

fn bench_entity_graph(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let graph = EntityGraph::new();

    c.bench_function("entity_graph_upsert", |b| {
        b.to_async(&rt).iter(|| async {
            graph
                .upsert_memory("m1", "Alice works at Acme in London", None)
                .await
                .unwrap();
        });
    });

    c.bench_function("entity_graph_traversal", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = graph
                .relations_for_entity("Alice", 2, None, xavier::memory::entity_graph::GraphDirection::Both)
                .await
                .unwrap();
        });
    });

    c.bench_function("entity_graph_inference", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = graph.run_inference().await.unwrap();
        });
    });

    c.bench_function("entity_graph_export_json", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = graph.export_json().await.unwrap();
        });
    });
}

criterion_group!(entity_graph_benches, bench_entity_graph);
criterion_main!(entity_graph_benches);
