//! Cross-platform subprocess ownership for bundled sidecars.

use tokio::process::{Child, Command};
use tracing::warn;

/// Keep sidecars invisible on Windows and isolated in their own process group
/// on Unix so cancellation also reaches subprocesses such as FFmpeg.
pub(crate) fn configure(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    #[cfg(unix)]
    {
        command.process_group(0);
    }
}

/// Terminate the complete sidecar process tree, falling back to the direct
/// child if the platform tree operation is unavailable or has already raced
/// with normal process exit.
pub(crate) async fn terminate_tree(child: &mut Child, name: &str) {
    let Some(pid) = child.id() else {
        let _ = child.kill().await;
        return;
    };

    #[cfg(windows)]
    let tree_signalled = {
        use std::os::windows::process::CommandExt;
        let result = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(0x08000000)
            .output()
            .await;
        match result {
            Ok(output) if output.status.success() => true,
            Ok(output) => {
                warn!(
                    sidecar = name,
                    pid,
                    exit_code = ?output.status.code(),
                    stderr = %String::from_utf8_lossy(&output.stderr),
                    "Could not terminate sidecar process tree"
                );
                false
            }
            Err(error) => {
                warn!(sidecar = name, pid, %error, "Could not terminate sidecar process tree");
                false
            }
        }
    };

    #[cfg(unix)]
    let tree_signalled = {
        if let Ok(process_group) = i32::try_from(pid) {
            // SAFETY: the child was placed in a process group whose id is its
            // pid immediately before spawn. A negative pid addresses that
            // group and SIGKILL requires no shared memory or signal handler.
            if unsafe { libc::kill(-process_group, libc::SIGKILL) } == 0 {
                true
            } else {
                warn!(
                    sidecar = name,
                    pid,
                    error = %std::io::Error::last_os_error(),
                    "Could not terminate sidecar process group"
                );
                false
            }
        } else {
            false
        }
    };

    if tree_signalled {
        let _ = child.wait().await;
    } else {
        let _ = child.kill().await;
    }
}
