use std::time::Duration;

use costae::spawn_module;

/// Verify that dropping a `SpawnedModule` kills the child process.
///
/// Strategy: spawn `sleep 60`, record its PID, drop the struct, wait a short
/// time for the OS to reap, then call `kill(pid, 0)` (signal 0 = probe only).
/// If the process is gone, `kill` returns -1 with errno == ESRCH.
#[test]
fn spawned_module_drop_kills_child() {
    // Use `sh` with a script that sleeps for a long time, so the child stays
    // alive until it is explicitly killed.
    let spawned = spawn_module("sh", Some("sleep 60"));
    let pid = spawned.child.id() as libc::pid_t;

    // Sanity: process must be alive before we drop.
    let alive_before = unsafe { libc::kill(pid, 0) } == 0;
    assert!(
        alive_before,
        "child process should be alive right after spawn"
    );

    drop(spawned);

    // Give the OS a moment to reap the child.
    std::thread::sleep(Duration::from_millis(100));

    let ret = unsafe { libc::kill(pid, 0) };
    if ret == 0 {
        panic!("child process (pid {pid}) is still alive after SpawnedModule was dropped");
    }
    let errno = unsafe { *libc::__errno_location() };
    assert_eq!(
        errno,
        libc::ESRCH,
        "expected ESRCH (no such process) after drop, got errno={errno}"
    );
}
