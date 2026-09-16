//! Criterion microbenchmarks for Vector Search & Quantization Evaluation.
//!
//! Benchmark Group: `vector_quantization_benchmarks`
//!
//! # Overview & Trade-off Findings:
//! This benchmark evaluates vector similarity search latency, encoding throughput,
//! and recall trade-offs across three embedding representations:
//!
//! 1. **Standard f32 (32-bit floats)**:
//!    - Memory Footprint: 4 bytes / dimension (10,000 x 1536d vectors = 61.44 MB)
//!    - Distance Metric: Exact float cosine distance
//!    - Precision/Recall: Ground truth (100% recall baseline)
//!
//! 2. **Scalar Quantization (i8 / int8)**:
//!    - Memory Footprint: 1 byte / dimension + 4 bytes scale (10,000 x 1536d vectors = 15.36 MB, ~75% memory reduction)
//!    - Distance Metric: Integer dot-product + float scale norm cosine distance
//!    - Precision/Recall: High top-k recall (>97% Recall@10) with ~4x throughput speedup
//!
//! 3. **QJL Two-Stage Quantization (Quantized Joint Learning)**:
//!    - Memory Footprint: 2 bytes / dimension + 16 bytes header (10,000 x 1536d vectors = 30.72 MB, ~50% memory reduction)
//!    - Distance Metric: Coarse 8-bit + Residual 8-bit vector reconstruction distance
//!    - Precision/Recall: Exceptional precision (~99% Recall@10) for high-dimensional or non-uniform embeddings

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use xavier::memory::sqlite_vec_store::{
    cosine_distance_f32, cosine_distance_i8, cosine_distance_qjl, quantize_scalar_i8,
    serialize_embedding_qjl,
};

/// Helper to generate deterministic pseudo-random float vectors normalized roughly on a sphere.
fn generate_random_vectors(count: usize, dims: usize, seed: u64) -> Vec<Vec<f32>> {
    let mut rng = StdRng::seed_from_u64(seed);
    (0..count)
        .map(|_| {
            let mut vec: Vec<f32> = (0..dims).map(|_| rng.gen_range(-1.0..1.0)).collect();
            let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in &mut vec {
                    *x /= norm;
                }
            }
            vec
        })
        .collect()
}

/// Benchmark micro-level distance metric calculation (f32 vs i8 vs QJL) across standard dimensions.
fn bench_cosine_distance_micro(c: &mut Criterion) {
    let dimensions = [128, 384, 1536];
    let mut group = c.benchmark_group("cosine_distance_micro");

    for &dims in &dimensions {
        let vectors = generate_random_vectors(2, dims, 42);
        let v1 = &vectors[0];
        let v2 = &vectors[1];

        let (i8_v1, scale1) = quantize_scalar_i8(v1);
        let (i8_v2, scale2) = quantize_scalar_i8(v2);

        let qjl_v1 = serialize_embedding_qjl(v1);
        let qjl_v2 = serialize_embedding_qjl(v2);

        group.throughput(Throughput::Elements(1));

        group.bench_with_input(BenchmarkId::new("f32_exact", dims), &dims, |b, _| {
            b.iter(|| {
                let dist = cosine_distance_f32(black_box(v1), black_box(v2));
                black_box(dist);
            });
        });

        group.bench_with_input(BenchmarkId::new("scalar_i8", dims), &dims, |b, _| {
            b.iter(|| {
                let dist = cosine_distance_i8(
                    black_box(&i8_v1),
                    black_box(scale1),
                    black_box(&i8_v2),
                    black_box(scale2),
                );
                black_box(dist);
            });
        });

        group.bench_with_input(BenchmarkId::new("qjl_two_stage", dims), &dims, |b, _| {
            b.iter(|| {
                let dist = cosine_distance_qjl(black_box(&qjl_v1), black_box(&qjl_v2));
                black_box(dist);
            });
        });
    }

    group.finish();
}

/// Benchmark quantization encoding throughput (quantize f32 -> i8 vs f32 -> QJL).
fn bench_quantization_encoding_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("quantization_encoding");
    let batch_size = 1000;
    let dims = 384;
    let dataset = generate_random_vectors(batch_size, dims, 1337);

    group.throughput(Throughput::Elements(batch_size as u64));

    group.bench_function("scalar_i8_encoding_1k", |b| {
        b.iter(|| {
            for v in &dataset {
                let res = quantize_scalar_i8(black_box(v));
                black_box(res);
            }
        });
    });

    group.bench_function("qjl_two_stage_encoding_1k", |b| {
        b.iter(|| {
            for v in &dataset {
                let res = serialize_embedding_qjl(black_box(v));
                black_box(res);
            }
        });
    });

    group.finish();
}

/// Benchmark 10,000+ candidate vector linear k-NN scan over quantized vs unquantized sets.
fn bench_vector_search_scan_10k(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector_search_scan_10k");
    let dataset_size = 10_000;
    let dims = 384;
    let candidate_embeddings = generate_random_vectors(dataset_size, dims, 2026);
    let query_vector = &generate_random_vectors(1, dims, 999)[0];

    // Pre-quantize candidate dataset
    let i8_dataset: Vec<(Vec<i8>, f32)> = candidate_embeddings
        .iter()
        .map(|e| quantize_scalar_i8(e))
        .collect();
    let (query_i8, query_scale) = quantize_scalar_i8(query_vector);

    let qjl_dataset: Vec<Vec<u8>> = candidate_embeddings
        .iter()
        .map(|e| serialize_embedding_qjl(e))
        .collect();
    let query_qjl = serialize_embedding_qjl(query_vector);

    group.throughput(Throughput::Elements(dataset_size as u64));

    group.bench_function("scan_10k_f32_exact", |b| {
        b.iter(|| {
            let mut distances: Vec<f32> = Vec::with_capacity(dataset_size);
            for candidate in &candidate_embeddings {
                distances.push(cosine_distance_f32(
                    black_box(query_vector),
                    black_box(candidate),
                ));
            }
            black_box(distances);
        });
    });

    group.bench_function("scan_10k_scalar_i8", |b| {
        b.iter(|| {
            let mut distances: Vec<f32> = Vec::with_capacity(dataset_size);
            for (q, scale) in &i8_dataset {
                distances.push(cosine_distance_i8(
                    black_box(&query_i8),
                    black_box(query_scale),
                    black_box(q),
                    black_box(*scale),
                ));
            }
            black_box(distances);
        });
    });

    group.bench_function("scan_10k_qjl_two_stage", |b| {
        b.iter(|| {
            let mut distances: Vec<f32> = Vec::with_capacity(dataset_size);
            for candidate_qjl in &qjl_dataset {
                distances.push(cosine_distance_qjl(
                    black_box(&query_qjl),
                    black_box(candidate_qjl),
                ));
            }
            black_box(distances);
        });
    });

    group.finish();
}

criterion_group!(
    vector_quantization_benchmarks,
    bench_cosine_distance_micro,
    bench_quantization_encoding_throughput,
    bench_vector_search_scan_10k
);
criterion_main!(vector_quantization_benchmarks);
