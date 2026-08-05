use super::*;

#[test]
fn unknown_state_display() {
    let state = ResourceState::Unknown {
        reason: "env var not set".to_string(),
    };
    assert_eq!(state.to_string(), "unknown (env var not set)");
}
