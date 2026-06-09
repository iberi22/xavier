use super::router::*;

#[test]
fn test_router_initialization() {
    let router = ProviderRouter::new(ProviderKind::Local);
    assert_eq!(router.current_provider(), ProviderKind::Local);
    assert_eq!(router.select(), ProviderKind::Local);
    assert_eq!(*router.active_mode(), ActiveProvider::Manual(ProviderKind::Local));
    assert_eq!(router.history().len(), 1);
}

#[test]
fn test_manual_switch() {
    let mut router = ProviderRouter::new(ProviderKind::Local);
    router.switch_to(ProviderKind::OpenAI).unwrap();
    assert_eq!(router.current_provider(), ProviderKind::OpenAI);
    assert_eq!(router.select(), ProviderKind::OpenAI);
    assert_eq!(router.history().len(), 2);
    assert_eq!(router.history()[1].reason, "Manual switch");
}

#[test]
fn test_fallback_on_failure() {
    let mut router = ProviderRouter::new(ProviderKind::OpenAI);
    router.set_fallback_chain(vec![
        ProviderKind::OpenAI,
        ProviderKind::Anthropic,
        ProviderKind::Local,
    ]);

    // Current is OpenAI (index 0).
    // First failure: should transition to Fallback mode and pick index 1 (Anthropic)
    let next = router.fallback();
    assert_eq!(next, Some(ProviderKind::Anthropic));
    assert_eq!(router.current_provider(), ProviderKind::Anthropic);
    assert!(matches!(router.active_mode(), ActiveProvider::Fallback(_)));

    // Second failure: should pick index 2 (Local)
    let next = router.fallback();
    assert_eq!(next, Some(ProviderKind::Local));
    assert_eq!(router.current_provider(), ProviderKind::Local);

    // Third failure: no more fallbacks
    let next = router.fallback();
    assert_eq!(next, None);
}

#[test]
fn test_auto_strategy_setting() {
    let mut router = ProviderRouter::new(ProviderKind::Local);
    router.set_auto_strategy(AutoStrategy::LowestCost);
    assert_eq!(*router.active_mode(), ActiveProvider::Auto { strategy: AutoStrategy::LowestCost });
}

#[test]
fn test_use_fallback_mode() {
    let mut router = ProviderRouter::new(ProviderKind::OpenAI);
    router.set_fallback_chain(vec![
        ProviderKind::Anthropic,
        ProviderKind::Local,
    ]);

    router.use_fallback_mode();
    assert_eq!(router.current_provider(), ProviderKind::Anthropic);
    assert!(matches!(router.active_mode(), ActiveProvider::Fallback(_)));
}

#[test]
fn test_on_provider_failure_without_chain() {
    let mut router = ProviderRouter::new(ProviderKind::OpenAI);
    let next = router.on_provider_failure();
    assert_eq!(next, None);
    assert_eq!(router.current_provider(), ProviderKind::OpenAI);
}
