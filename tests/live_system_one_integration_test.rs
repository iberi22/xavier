use std::time::Instant;
use xavier::agents::fast_router_poc::{FastRouter, Route, SystemOneDecision};

#[test]
fn test_live_system_one_scenarios_and_benchmark() {
    let router = FastRouter::new(0.90);

    // Escenario 1: Salida mecánica de tool exitosa sin errores
    let start = Instant::now();
    let step1 = router.triage("Tool execution finished with exit code 0", true, false);
    let duration1 = start.elapsed();
    assert_eq!(step1.route, Route::FastLane);
    assert!(step1.is_mechanical);
    assert!(!step1.has_pii);
    assert!(step1.confidence >= 0.90);
    println!("⏱️ Latencia de triaje Escenario 1: {:?}", duration1);

    // Escenario 2: Salida con error de compilador (debe degradar a fail-open SystemTwo)
    let start = Instant::now();
    let step2 = router.triage(
        "error[E0425]: cannot find value `foo` in this scope",
        true,
        true,
    );
    let duration2 = start.elapsed();
    assert_eq!(step2.route, Route::SystemTwoLane);
    assert!(!step2.is_mechanical);
    assert!(step2.confidence < 0.90);
    println!("⏱️ Latencia de triaje Escenario 2: {:?}", duration2);

    // Escenario 3: Intercepción de PII / credenciales críticas
    let start = Instant::now();
    let step3 = router.triage("User password is SuperSecretPassword123!", false, false);
    let duration3 = start.elapsed();
    assert!(step3.has_pii);
    assert_eq!(step3.route, Route::SystemTwoLane);
    println!("⏱️ Latencia de triaje Escenario 3: {:?}", duration3);

    // Escenario 4: Benchmark de 100,000 evaluaciones secuenciales
    let bench_start = Instant::now();
    for _ in 0..100_000 {
        let _ = router.triage("Regular log line without issues", true, false);
    }
    let bench_elapsed = bench_start.elapsed();
    let avg_per_op = bench_elapsed.as_nanos() as f64 / 100_000.0;
    println!(
        "🚀 Rendimiento System One: 100.000 operaciones en {:?} (~{:.2} ns/op)",
        bench_elapsed, avg_per_op
    );
    assert!(
        avg_per_op < 1000.0,
        "La operación debe tomar menos de 1 microsegundo por turno"
    );
}
