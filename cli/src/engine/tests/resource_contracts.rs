use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::engine::{TaskResult, process_resources, process_resources_remove};
use crate::resources::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::tasks::test_helpers::empty_config;

use super::{bail_opts, default_opts, dry_run_context, test_context};

#[derive(Clone)]
struct ContractResource {
    state: Arc<Mutex<ResourceState>>,
    apply_calls: Arc<AtomicUsize>,
    remove_calls: Arc<AtomicUsize>,
}

impl ContractResource {
    fn new(state: ResourceState) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            apply_calls: Arc::new(AtomicUsize::new(0)),
            remove_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn state(&self) -> ResourceState {
        self.state.lock().unwrap().clone()
    }

    fn apply_calls(&self) -> usize {
        self.apply_calls.load(Ordering::SeqCst)
    }

    fn remove_calls(&self) -> usize {
        self.remove_calls.load(Ordering::SeqCst)
    }
}

impl Resource for ContractResource {
    fn description(&self) -> String {
        "contract resource".to_string()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        self.apply_calls.fetch_add(1, Ordering::SeqCst);
        *self.state.lock().unwrap() = ResourceState::Correct;
        Ok(ResourceChange::Applied)
    }

    fn remove(&self) -> ResourceResult<ResourceChange> {
        self.remove_calls.fetch_add(1, Ordering::SeqCst);
        *self.state.lock().unwrap() = ResourceState::Missing;
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for ContractResource {
    fn current_state(&self) -> anyhow::Result<ResourceState> {
        Ok(self.state())
    }
}

fn contract_context() -> crate::engine::Context {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = test_context(config);
    ctx
}

fn dry_run_contract_context() -> crate::engine::Context {
    let config = empty_config(PathBuf::from("/tmp"));
    let (ctx, _log) = dry_run_context(config);
    ctx
}

#[test]
fn contract_missing_resource_applies_once_then_noops() {
    let ctx = contract_context();
    let resource = ContractResource::new(ResourceState::Missing);
    let opts = bail_opts();

    let first = process_resources(&ctx, [resource.clone()], &opts).unwrap();
    let second = process_resources(&ctx, [resource.clone()], &opts).unwrap();

    assert!(matches!(first, TaskResult::Ok));
    assert!(matches!(second, TaskResult::Ok));
    assert_eq!(resource.state(), ResourceState::Correct);
    assert_eq!(resource.apply_calls(), 1);
}

#[test]
fn contract_incorrect_resource_repairs_once_then_noops() {
    let ctx = contract_context();
    let resource = ContractResource::new(ResourceState::Incorrect {
        current: "drifted".to_string(),
    });
    let opts = bail_opts();

    let first = process_resources(&ctx, [resource.clone()], &opts).unwrap();
    let second = process_resources(&ctx, [resource.clone()], &opts).unwrap();

    assert!(matches!(first, TaskResult::Ok));
    assert!(matches!(second, TaskResult::Ok));
    assert_eq!(resource.state(), ResourceState::Correct);
    assert_eq!(resource.apply_calls(), 1);
}

#[test]
fn contract_dry_run_never_applies_missing_or_incorrect_resources() {
    let ctx = dry_run_contract_context();
    let missing = ContractResource::new(ResourceState::Missing);
    let incorrect = ContractResource::new(ResourceState::Incorrect {
        current: "drifted".to_string(),
    });
    let opts = bail_opts();

    let result = process_resources(&ctx, [missing.clone(), incorrect.clone()], &opts).unwrap();

    assert!(matches!(result, TaskResult::DryRun));
    assert_eq!(missing.state(), ResourceState::Missing);
    assert_eq!(
        incorrect.state(),
        ResourceState::Incorrect {
            current: "drifted".to_string(),
        }
    );
    assert_eq!(missing.apply_calls(), 0);
    assert_eq!(incorrect.apply_calls(), 0);
}

#[test]
fn contract_invalid_and_unknown_resources_are_not_applied() {
    let ctx = contract_context();
    let invalid = ContractResource::new(ResourceState::Invalid {
        reason: "unsafe target".to_string(),
    });
    let unknown = ContractResource::new(ResourceState::Unknown {
        reason: "state probe failed".to_string(),
    });
    let opts = bail_opts();

    let result = process_resources(&ctx, [invalid.clone(), unknown.clone()], &opts).unwrap();

    assert!(matches!(result, TaskResult::Ok));
    assert_eq!(invalid.apply_calls(), 0);
    assert_eq!(unknown.apply_calls(), 0);
}

#[test]
fn contract_lenient_apply_errors_are_safe_skips() {
    let ctx = contract_context();
    let resource = super::MockResource::new(ResourceState::Missing).with_apply(Err("boom".into()));
    let opts = default_opts();

    let result = process_resources(&ctx, [resource], &opts).unwrap();

    assert!(matches!(result, TaskResult::Ok));
}

#[test]
fn contract_remove_correct_resource_once_then_noops() {
    let ctx = contract_context();
    let resource = ContractResource::new(ResourceState::Correct);

    let first = process_resources_remove(&ctx, [resource.clone()], "remove").unwrap();
    let second = process_resources_remove(&ctx, [resource.clone()], "remove").unwrap();

    assert!(matches!(first, TaskResult::Ok));
    assert!(matches!(second, TaskResult::Ok));
    assert_eq!(resource.state(), ResourceState::Missing);
    assert_eq!(resource.remove_calls(), 1);
}

#[test]
fn contract_remove_dry_run_never_mutates_correct_resources() {
    let ctx = dry_run_contract_context();
    let resource = ContractResource::new(ResourceState::Correct);

    let result = process_resources_remove(&ctx, [resource.clone()], "remove").unwrap();

    assert!(matches!(result, TaskResult::DryRun));
    assert_eq!(resource.state(), ResourceState::Correct);
    assert_eq!(resource.remove_calls(), 0);
}

#[test]
fn contract_remove_does_not_touch_unmanaged_or_unsafe_states() {
    let ctx = contract_context();
    let missing = ContractResource::new(ResourceState::Missing);
    let incorrect = ContractResource::new(ResourceState::Incorrect {
        current: "owned by user".to_string(),
    });
    let invalid = ContractResource::new(ResourceState::Invalid {
        reason: "unsafe target".to_string(),
    });
    let unknown = ContractResource::new(ResourceState::Unknown {
        reason: "state probe failed".to_string(),
    });

    let result = process_resources_remove(
        &ctx,
        [
            missing.clone(),
            incorrect.clone(),
            invalid.clone(),
            unknown.clone(),
        ],
        "remove",
    )
    .unwrap();

    assert!(matches!(result, TaskResult::Ok));
    assert_eq!(missing.remove_calls(), 0);
    assert_eq!(incorrect.remove_calls(), 0);
    assert_eq!(invalid.remove_calls(), 0);
    assert_eq!(unknown.remove_calls(), 0);
}
