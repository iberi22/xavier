use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use xavier::telecom::crypto::{
    derive_shared_secret, ratchet_decrypt, ratchet_encrypt, SessionRatchet, TelecomKeypair,
};

fn bench_encrypt_decrypt(c: &mut Criterion) {
    let mut group = c.benchmark_group("telecom_crypto_throughput");

    // Set up a consistent key and ratchet
    let alice = TelecomKeypair::generate();
    let bob = TelecomKeypair::generate();
    let shared_secret = alice.diffie_hellman(&bob.public_key_bytes());
    let derived_key = derive_shared_secret(&shared_secret, b"bench-context");

    let mut alice_ratchet = SessionRatchet::new(derived_key);
    let _bob_ratchet = SessionRatchet::new(derived_key);

    // Test sizes: 64B, 1KB, 64KB, 1MB
    let sizes = [64, 1024, 64 * 1024, 1024 * 1024];

    for size in sizes {
        group.throughput(Throughput::Bytes(size as u64));
        let payload: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        let channel = "bench-channel";

        group.bench_with_input(BenchmarkId::new("encrypt", size), &size, |b, &_s| {
            b.iter(|| {
                // To avoid sequence drift in benchmarking iter, we generate keys outside, or keep ratchet stepping.
                // We'll just step the ratchet here.
                let key = alice_ratchet.next_message_key();
                ratchet_encrypt(&key, channel, alice_ratchet.sequence(), &payload).unwrap()
            })
        });

        let key = alice_ratchet.next_message_key();
        let frame = ratchet_encrypt(&key, channel, alice_ratchet.sequence(), &payload).unwrap();

        group.bench_with_input(BenchmarkId::new("decrypt", size), &size, |b, &_s| {
            b.iter(|| ratchet_decrypt(&key, &frame).unwrap())
        });
    }

    group.finish();
}

fn bench_key_derivation(c: &mut Criterion) {
    let mut group = c.benchmark_group("telecom_key_derivation");

    group.bench_function("generate_keypair", |b| b.iter(TelecomKeypair::generate));

    let alice = TelecomKeypair::generate();
    let bob = TelecomKeypair::generate();

    group.bench_function("diffie_hellman", |b| {
        let bob_pub = bob.public_key_bytes();
        b.iter(|| alice.diffie_hellman(&bob_pub))
    });

    let shared = alice.diffie_hellman(&bob.public_key_bytes());

    group.bench_function("derive_shared_secret", |b| {
        b.iter(|| derive_shared_secret(&shared, b"bench-context"))
    });

    group.finish();
}

criterion_group!(benches, bench_encrypt_decrypt, bench_key_derivation);
criterion_main!(benches);
