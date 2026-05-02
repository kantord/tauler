use std::time::Duration;

use costae::spawn_module;

#[test]
fn spawn_module_receives_stdout_line_from_script() {
    let m = spawn_module("/usr/bin/bash", Some("echo hello"));
    let line = m.rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(line, "hello");
}

#[test]
fn spawn_module_receives_multiple_lines() {
    let m = spawn_module("/usr/bin/bash", Some("echo first\necho second"));
    let first = m.rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = m.rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(first, "first");
    assert_eq!(second, "second");
}

#[test]
fn spawn_module_works_without_script() {
    let m = spawn_module("/bin/echo", None);
    let line = m.rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(line, "");
}

#[test]
fn killing_child_stops_receiver() {
    let mut m = spawn_module(
        "/usr/bin/bash",
        Some("while true; do echo tick; sleep 1; done"),
    );
    m.rx.recv_timeout(Duration::from_secs(2)).unwrap();
    m.child.kill().unwrap();
    m.child.wait().unwrap();
    let result = m.rx.recv_timeout(Duration::from_secs(2));
    assert!(result.is_err());
}

#[test]
fn module_receives_event_on_stdin() {
    let script = r#"read -r line; echo "$line""#;
    let m = spawn_module("/usr/bin/bash", Some(script));
    m.send_event(&serde_json::json!({"type": "ping"}));
    let line = m.rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(parsed["type"], "ping");
}

#[test]
fn module_can_read_init_event_fields() {
    let script = r#"
        read -r line
        output=$(echo "$line" | /usr/bin/python3 -c "import sys,json; d=json.load(sys.stdin); print(d['output'])")
        width=$(echo "$line" | /usr/bin/python3 -c "import sys,json; d=json.load(sys.stdin); print(d['config']['width'])")
        echo "$output:$width"
    "#;
    let m = spawn_module("/usr/bin/bash", Some(script));
    m.send_event(&serde_json::json!({
        "type": "init",
        "output": "DP-1",
        "config": {"width": 200}
    }));
    let line = m.rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(line, "DP-1:200");
}
