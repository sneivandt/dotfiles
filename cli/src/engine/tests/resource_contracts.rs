use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::engine::{IntrinsicState, Resource, ResourceChange, ResourceResult, ResourceState};
use crate::engine::{ProcessOpts, TaskResult, process_resources, process_resources_remove};
use crate::test_helpers::empty_config;

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

#[derive(Clone)]
struct StateCase {
    name: &'static str,
    state: ResourceState,
}

struct ProcessModeCase {
    name: &'static str,
    opts: ProcessOpts,
    applies_missing: bool,
    applies_incorrect: bool,
}

fn state_cases() -> [StateCase; 5] {
    [
        StateCase {
            name: "missing",
            state: ResourceState::Missing,
        },
        StateCase {
            name: "correct",
            state: ResourceState::Correct,
        },
        StateCase {
            name: "incorrect",
            state: ResourceState::Incorrect {
                current: "drifted".to_string(),
            },
        },
        StateCase {
            name: "invalid",
            state: ResourceState::Invalid {
                reason: "unsafe target".to_string(),
            },
        },
        StateCase {
            name: "unknown",
            state: ResourceState::Unknown {
                reason: "state probe failed".to_string(),
            },
        },
    ]
}

fn process_mode_cases() -> [ProcessModeCase; 4] {
    [
        ProcessModeCase {
            name: "strict",
            opts: ProcessOpts::strict("install"),
            applies_missing: true,
            applies_incorrect: true,
        },
        ProcessModeCase {
            name: "lenient",
            opts: ProcessOpts::lenient("install"),
            applies_missing: true,
            applies_incorrect: true,
        },
        ProcessModeCase {
            name: "install-missing",
            opts: ProcessOpts::install_missing("install"),
            applies_missing: true,
            applies_incorrect: false,
        },
        ProcessModeCase {
            name: "fix-existing",
            opts: ProcessOpts::fix_existing("configure"),
            applies_missing: false,
            applies_incorrect: true,
        },
    ]
}

fn mode_applies_state(mode: &ProcessModeCase, state: &ResourceState) -> bool {
    match state {
        ResourceState::Missing => mode.applies_missing,
        ResourceState::Incorrect { .. } => mode.applies_incorrect,
        ResourceState::Correct | ResourceState::Invalid { .. } | ResourceState::Unknown { .. } => {
            false
        }
    }
}

fn state_reports_nonfatal_failure(state: &ResourceState) -> bool {
    matches!(
        state,
        ResourceState::Invalid { .. } | ResourceState::Unknown { .. }
    )
}

const fn batch_changed(result: &TaskResult) -> bool {
    matches!(result, TaskResult::Batch(stats) if stats.changed > 0)
}

const fn batch_failed(result: &TaskResult) -> bool {
    matches!(result, TaskResult::Batch(stats) if stats.failed > 0)
}

const fn batch_unchanged(result: &TaskResult) -> bool {
    matches!(
        result,
        TaskResult::Batch(stats) if stats.changed == 0 && stats.failed == 0
    )
}

#[test]
fn contract_missing_resource_applies_once_then_noops() {
    let ctx = contract_context();
    let resource = ContractResource::new(ResourceState::Missing);
    let opts = bail_opts();

    let first = process_resources(&ctx, [resource.clone()], &opts).unwrap();
    let second = process_resources(&ctx, [resource.clone()], &opts).unwrap();

    assert!(batch_changed(&first));
    assert!(batch_unchanged(&second));
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

    assert!(batch_changed(&first));
    assert!(batch_unchanged(&second));
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

    assert!(batch_changed(&result));
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

    assert!(batch_failed(&result));
    assert_eq!(invalid.apply_calls(), 0);
    assert_eq!(unknown.apply_calls(), 0);
}

#[test]
fn contract_lenient_apply_errors_are_nonfatal_failures() {
    let ctx = contract_context();
    let resource = super::MockResource::new(ResourceState::Missing).with_apply(Err("boom".into()));
    let opts = default_opts();

    let result = process_resources(&ctx, [resource], &opts).unwrap();

    assert!(batch_failed(&result));
}

#[test]
fn contract_remove_correct_resource_once_then_noops() {
    let ctx = contract_context();
    let resource = ContractResource::new(ResourceState::Correct);

    let first = process_resources_remove(&ctx, [resource.clone()], "remove").unwrap();
    let second = process_resources_remove(&ctx, [resource.clone()], "remove").unwrap();

    assert!(batch_changed(&first));
    assert!(batch_unchanged(&second));
    assert_eq!(resource.state(), ResourceState::Missing);
    assert_eq!(resource.remove_calls(), 1);
}

#[test]
fn contract_remove_dry_run_never_mutates_correct_resources() {
    let ctx = dry_run_contract_context();
    let resource = ContractResource::new(ResourceState::Correct);

    let result = process_resources_remove(&ctx, [resource.clone()], "remove").unwrap();

    assert!(batch_changed(&result));
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

    assert!(batch_unchanged(&result));
    assert_eq!(missing.remove_calls(), 0);
    assert_eq!(incorrect.remove_calls(), 0);
    assert_eq!(invalid.remove_calls(), 0);
    assert_eq!(unknown.remove_calls(), 0);
}

#[test]
fn contract_process_modes_apply_only_their_fixable_states() -> anyhow::Result<()> {
    let ctx = contract_context();

    for mode in process_mode_cases() {
        for case in state_cases() {
            let resource = ContractResource::new(case.state.clone());
            let should_apply = mode_applies_state(&mode, &case.state);

            let result = process_resources(&ctx, [resource.clone()], &mode.opts)?;

            let expected_failure = state_reports_nonfatal_failure(&case.state);
            assert_eq!(
                batch_failed(&result),
                expected_failure,
                "mode {} and state {} should report failure only for unsafe states",
                mode.name,
                case.name
            );
            assert_eq!(
                resource.apply_calls(),
                usize::from(should_apply),
                "mode {} should apply state {} exactly as declared by its contract",
                mode.name,
                case.name
            );
            assert_eq!(
                resource.remove_calls(),
                0,
                "install processing must not call remove for mode {} and state {}",
                mode.name,
                case.name
            );

            let expected_state = if should_apply {
                ResourceState::Correct
            } else {
                case.state
            };
            assert_eq!(
                resource.state(),
                expected_state,
                "mode {} should leave state {} in the expected post-condition",
                mode.name,
                case.name
            );
        }
    }

    Ok(())
}

#[test]
fn contract_dry_run_process_modes_never_mutate_any_state() -> anyhow::Result<()> {
    let ctx = dry_run_contract_context();

    for mode in process_mode_cases() {
        for case in state_cases() {
            let resource = ContractResource::new(case.state.clone());

            let result = process_resources(&ctx, [resource.clone()], &mode.opts)?;
            let would_apply = mode_applies_state(&mode, &case.state);

            assert!(
                batch_changed(&result) == would_apply,
                "dry-run mode {} and state {} should report dry-run only when it would apply",
                mode.name,
                case.name
            );
            assert_eq!(
                resource.state(),
                case.state,
                "dry-run mode {} should not mutate state {}",
                mode.name,
                case.name
            );
            assert_eq!(
                resource.apply_calls(),
                0,
                "dry-run mode {} should not apply state {}",
                mode.name,
                case.name
            );
            assert_eq!(
                resource.remove_calls(),
                0,
                "dry-run mode {} should not remove state {}",
                mode.name,
                case.name
            );
        }
    }

    Ok(())
}

#[test]
fn contract_remove_only_mutates_correct_resources() -> anyhow::Result<()> {
    let ctx = contract_context();

    for case in state_cases() {
        let resource = ContractResource::new(case.state.clone());
        let should_remove = matches!(case.state, ResourceState::Correct);

        let result = process_resources_remove(&ctx, [resource.clone()], "remove")?;

        if should_remove {
            assert!(
                batch_changed(&result),
                "remove state {} should report a change",
                case.name
            );
        } else {
            assert!(
                batch_unchanged(&result),
                "remove state {} should complete without a change",
                case.name
            );
        }
        assert_eq!(
            resource.remove_calls(),
            usize::from(should_remove),
            "remove should call remove exactly once only for correct resources"
        );
        assert_eq!(
            resource.apply_calls(),
            0,
            "remove should never call apply for state {}",
            case.name
        );

        let expected_state = if should_remove {
            ResourceState::Missing
        } else {
            case.state
        };
        assert_eq!(
            resource.state(),
            expected_state,
            "remove should leave state {} in the expected post-condition",
            case.name
        );
    }

    Ok(())
}

#[test]
fn contract_remove_dry_run_never_mutates_any_state() -> anyhow::Result<()> {
    let ctx = dry_run_contract_context();

    for case in state_cases() {
        let resource = ContractResource::new(case.state.clone());

        let result = process_resources_remove(&ctx, [resource.clone()], "remove")?;
        let would_remove = matches!(case.state, ResourceState::Correct);

        assert!(
            batch_changed(&result) == would_remove,
            "dry-run remove state {} should report dry-run only when it would remove",
            case.name
        );
        assert_eq!(
            resource.state(),
            case.state,
            "dry-run remove should not mutate state {}",
            case.name
        );
        assert_eq!(
            resource.remove_calls(),
            0,
            "dry-run remove should not call remove for state {}",
            case.name
        );
        assert_eq!(
            resource.apply_calls(),
            0,
            "dry-run remove should not call apply for state {}",
            case.name
        );
    }

    Ok(())
}
