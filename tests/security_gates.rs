use core::security::gates::{evaluate_gates, ActionPayload, GateError};

#[test]
fn test_gates_drop_task_on_failure() {
    let payload = ActionPayload {
        is_file_op: true,
        is_command: false,
        command_str: "".into(),
        path: Some("../secret".into()),
    };

    let result = evaluate_gates(&payload);
    assert!(result.is_err());
    
    // Hard drop check:
    match result {
        Err(GateError::FileSystemViolation(_)) => {}
        _ => panic!("Expected FileSystemViolation"),
    }
}
