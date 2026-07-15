use anyhow::Result;

use super::*;

struct TestResource {
    state: ResourceState,
}

impl Resource for TestResource {
    fn description(&self) -> String {
        "test resource".to_string()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for TestResource {
    fn current_state(&self) -> Result<ResourceState> {
        Ok(self.state.clone())
    }
}

#[test]
fn needs_change_for_missing_resource() {
    let resource = TestResource {
        state: ResourceState::Missing,
    };
    assert!(resource.needs_change().unwrap());
}

#[test]
fn needs_change_for_incorrect_resource() {
    let resource = TestResource {
        state: ResourceState::Incorrect {
            current: "wrong".to_string(),
        },
    };
    assert!(resource.needs_change().unwrap());
}

#[test]
fn no_change_for_correct_resource() {
    let resource = TestResource {
        state: ResourceState::Correct,
    };
    assert!(!resource.needs_change().unwrap());
}

#[test]
fn no_change_for_invalid_resource() {
    let resource = TestResource {
        state: ResourceState::Invalid {
            reason: "directory exists".to_string(),
        },
    };
    assert!(!resource.needs_change().unwrap());
}

#[test]
fn no_change_for_unknown_resource() {
    let resource = TestResource {
        state: ResourceState::Unknown {
            reason: "detection tool unavailable".to_string(),
        },
    };
    assert!(!resource.needs_change().unwrap());
}

#[test]
fn unknown_state_display() {
    let state = ResourceState::Unknown {
        reason: "env var not set".to_string(),
    };
    assert_eq!(state.to_string(), "unknown (env var not set)");
}

#[test]
fn default_remove_returns_error() {
    let resource = TestResource {
        state: ResourceState::Correct,
    };
    let err = resource.remove().unwrap_err();
    assert!(
        err.to_string().contains("not supported"),
        "expected 'not supported' in: {err}"
    );
    assert!(
        err.to_string().contains("test resource"),
        "expected resource description in: {err}"
    );
}
