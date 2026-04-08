use std::env;
use std::process::Command;

#[test]
fn test_help_command() {
    let mut path = env::current_exe().unwrap();
    path.pop(); // Pop off the test executable
    path.pop(); // Pop off deps
    path.push("mempalace-rs");

    let output = Command::new(path)
        .arg("--help")
        .output()
        .expect("failed to execute process");
    assert!(output.status.success());
    let help_text = String::from_utf8_lossy(&output.stdout);
    assert!(help_text.contains("Usage: mempalace-rs"));
}
