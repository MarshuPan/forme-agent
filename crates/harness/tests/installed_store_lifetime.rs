//! Shared production composition must retain the access fence, including callers
//! that retain the public Harness after dropping the runtime facade.
#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod core_brain;
use core_brain::Fixture;
use forme_store::StoreAccessGuard;

#[test]
fn production_harness_retains_shared_store_access_until_its_last_owner_drops() {
    let fixture = Fixture::new();
    let path = fixture.config.store_path.clone();
    let runtime = fixture.build();
    let services = runtime.services();
    let authority = services.authority_guard.clone();
    assert!(StoreAccessGuard::offline(&path).is_err());
    let harness = runtime.harness();
    drop(runtime);
    assert!(StoreAccessGuard::offline(&path).is_err());
    drop(harness);
    assert!(StoreAccessGuard::offline(&path).is_err());
    drop(services);
    assert!(StoreAccessGuard::offline(&path).is_err());
    drop(authority);
    let offline = StoreAccessGuard::offline(&path).unwrap();
    assert!(fixture.config.clone().validate().is_ok());
    assert!(
        forme_harness::ProductionRuntimeBuilder::from_config(fixture.config.clone())
            .unwrap()
            .build()
            .is_err()
    );
    drop(offline);
}
