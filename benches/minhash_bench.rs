use criterion::{black_box, criterion_group, criterion_main, Criterion};
use xavier::memory::qmd::{compute_minhash, jaccard_similarity, MemoryDocument};
use xavier::consolidation::merger::similarity;
use rand::Rng;

fn generate_random_text(size: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..size)
        .map(|_| {
            let idx = rng.gen_range(0..26);
            (b'a' + idx) as char
        })
        .collect()
}

fn minhash_bench(c: &mut Criterion) {
    let text1 = generate_random_text(1000);
    let text2 = generate_random_text(1000);

    let sig1 = compute_minhash(&text1);
    let sig2 = compute_minhash(&text2);

    c.bench_function("minhash_similarity", |b| {
        b.iter(|| jaccard_similarity(black_box(&sig1), black_box(&sig2)))
    });
}

fn cosine_similarity_bench(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    let vec1: Vec<f32> = (0..768).map(|_| rng.gen()).collect();
    let vec2: Vec<f32> = (0..768).map(|_| rng.gen()).collect();

    let doc1 = MemoryDocument {
        content_vector: Some(vec1),
        ..Default::default()
    };
    let doc2 = MemoryDocument {
        content_vector: Some(vec2),
        ..Default::default()
    };

    c.bench_function("cosine_similarity_via_merger", |b| {
        b.iter(|| similarity(black_box(&doc1), black_box(&doc2)))
    });
}

fn bulk_comparison_bench(c: &mut Criterion) {
    let num_docs = 1000;
    let mut docs = Vec::new();
    let mut rng = rand::thread_rng();

    for _ in 0..num_docs {
        let text = generate_random_text(500);
        let vec: Vec<f32> = (0..768).map(|_| rng.gen()).collect();
        let mut doc = MemoryDocument {
            content: text,
            content_vector: Some(vec),
            ..Default::default()
        };
        doc.minhash = Some(compute_minhash(&doc.content));
        docs.push(doc);
    }

    c.bench_function("bulk_minhash_prefilter", |b| {
        b.iter(|| {
            for i in 0..num_docs {
                for j in i + 1..num_docs {
                    let sim = jaccard_similarity(
                        docs[i].minhash.as_ref().unwrap(),
                        docs[j].minhash.as_ref().unwrap()
                    );
                    black_box(sim);
                }
            }
        })
    });

    c.bench_function("bulk_cosine_only", |b| {
        b.iter(|| {
            for i in 0..num_docs {
                for j in i + 1..num_docs {
                    // We bypass the merger's similarity here to measure raw cosine speed
                    // because the merger's similarity now includes MinHash.
                    let sim = xavier::memory::qmd_memory::cosine_similarity(
                        docs[i].content_vector.as_ref().unwrap(),
                        docs[j].content_vector.as_ref().unwrap()
                    );
                    black_box(sim);
                }
            }
        })
    });
}

criterion_group!(benches, minhash_bench, cosine_similarity_bench, bulk_comparison_bench);
criterion_main!(benches);
