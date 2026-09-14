#![cfg(unix)]
use cccc_runtime::OwnedProcessTree;
use std::process::{Command, Stdio};

#[test]
fn stopping_a_process_racing_with_stdin_eof_reaps_its_owned_child() {
    for attempt in 0..1000 {
        let (mut child, tree) = OwnedProcessTree::spawn(
            Command::new("sh")
                .args(["-c", "read line"])
                .stdin(Stdio::piped()),
        )
        .expect("spawn EOF fixture");
        drop(child.stdin.take());
        std::thread::sleep(std::time::Duration::from_micros(500));
        let result = (|| {
            if tree.try_wait(|| child.try_wait())?.is_none() {
                tree.request_stop()?;
            }
            tree.terminate()?;
            child.wait()?;
            std::io::Result::Ok(())
        })();
        // Reap even if the assertion fails, keeping a failed regression self-contained.
        if result.is_err() {
            let _ = child.kill();
            let _ = tree.terminate();
            let _ = child.wait();
        }
        result.unwrap_or_else(|error| panic!("stop attempt {attempt}: {error}"));
    }
}
