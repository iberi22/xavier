use std::io::Cursor;
use xavier::secrets::import_env::{parse_env_reader, read_value_from_reader};

#[test]
fn test_integration_secrets_import_env() {
    let mut env_cursor = Cursor::new("KEY_A=val_a\nKEY_B=val_b\n");
    let plan = parse_env_reader(&mut env_cursor).expect("failed to parse env");
    let names: Vec<&str> = plan.entries().iter().map(|e| e.name()).collect();
    assert_eq!(names, ["KEY_A", "KEY_B"]);

    let mut val_cursor = Cursor::new("synth_val\n");
    let val = read_value_from_reader(&mut val_cursor).expect("failed to read value");
    let expected = "synth_val";
    assert!(
        val.expose() == expected,
        "parsed value differs from expected"
    );
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
