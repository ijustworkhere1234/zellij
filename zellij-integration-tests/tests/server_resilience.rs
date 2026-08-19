#![cfg(unix)]

//! The IPC listener must outlive a failed connection attempt.
//!
//! `server_listener` loops over `listener.incoming()`, which calls `accept()` once per item. An
//! error from a single `accept()` says nothing about the health of the listening socket, but the
//! server used to panic on it. In a real session that panic is routed to
//! `ServerInstruction::Error`, which kicks every client and breaks the server loop, so one refused
//! connection destroyed every pane on that server.
//!
//! These tests drive the real `accept()` into its error arm by holding the process at its file
//! descriptor ceiling, then assert the session is still usable afterwards.

use std::os::unix::net::UnixStream;
use std::time::Duration;

use zellij_integration_tests::{
    claim_first_terminal_and_wait_for_prompt, TestRunner, TERMINAL_SIZE,
};
use zellij_utils::consts::ZELLIJ_SOCK_DIR;

/// Holds the process at its file descriptor ceiling for as long as it is alive.
///
/// `accept()` needs a free descriptor to return a connection, so while this is held any incoming
/// connection makes the listener's `accept()` fail with EMFILE — the same error arm any other
/// transient accept failure lands in. Dropping it restores the previous limit.
struct FdCeiling {
    hogs: Vec<std::fs::File>,
    previous_soft_limit: libc::rlim_t,
}

impl FdCeiling {
    /// Lower RLIMIT_NOFILE to just above what the process is already using, then consume every
    /// remaining descriptor.
    fn apply() -> Self {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: `limit` is a valid, fully initialized rlimit for the duration of the call.
        assert_eq!(
            unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
            0,
            "could not read RLIMIT_NOFILE"
        );
        let previous_soft_limit = limit.rlim_cur;

        // A ceiling low enough to reach quickly, high enough that the already-running server
        // threads keep the descriptors they are holding.
        let ceiling = 256;
        let lowered = libc::rlimit {
            rlim_cur: ceiling,
            rlim_max: limit.rlim_max,
        };
        // SAFETY: `lowered` is a valid rlimit and the soft limit stays below the hard limit.
        assert_eq!(
            unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lowered) },
            0,
            "could not lower RLIMIT_NOFILE"
        );

        let mut hogs = Vec::new();
        while let Ok(file) = std::fs::File::open("/dev/null") {
            hogs.push(file);
        }
        assert!(
            !hogs.is_empty(),
            "expected to claim at least one descriptor before hitting the ceiling"
        );
        FdCeiling {
            hogs,
            previous_soft_limit,
        }
    }

    /// Release exactly one descriptor so the caller can open a single socket, keeping the table
    /// full again the moment that socket is created.
    fn release_one(&mut self) {
        self.hogs.pop();
    }
}

impl Drop for FdCeiling {
    fn drop(&mut self) {
        self.hogs.clear();
        let restored = libc::rlimit {
            rlim_cur: self.previous_soft_limit,
            rlim_max: self.previous_soft_limit,
        };
        // SAFETY: restoring a limit we previously read from the same process.
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &restored) };
    }
}

#[test]
fn session_survives_a_failed_accept() {
    let mut zellij = TestRunner::new(TERMINAL_SIZE).start();
    claim_first_terminal_and_wait_for_prompt(&zellij);

    let socket_path = ZELLIJ_SOCK_DIR.join(zellij.session_name());
    assert!(
        socket_path.exists(),
        "expected the session socket at {:?}",
        socket_path
    );

    {
        let mut ceiling = FdCeiling::apply();
        // Free a single slot so the connect below can allocate its socket; once it does, the
        // table is full again and the server's accept() for it has nowhere to put the connection.
        ceiling.release_one();
        let _refused = UnixStream::connect(&socket_path);

        // Guard against a vacuous pass: if the process were not actually at its ceiling here,
        // accept() would succeed and this test would prove nothing.
        let spare = std::fs::File::open("/dev/null");
        assert!(
            matches!(
                spare.as_ref().map_err(|e| e.raw_os_error()),
                Err(Some(libc::EMFILE))
            ),
            "expected the descriptor table to be full so the server's accept() fails, \
             but opening another file returned {:?}",
            spare.map(|_| "success")
        );

        // Let the listener wake up, call accept(), and fail.
        std::thread::sleep(Duration::from_millis(250));
    }

    // The listener thread must still be alive and accepting. Before the fix it panicked on the
    // failed accept and this attach hangs until the harness times out.
    let second_client = zellij.attach_client(TERMINAL_SIZE);
    second_client.wait_until(
        "a new client can still attach after the listener refused a connection",
        |grid_snapshot| grid_snapshot.tab_bar_appears() && grid_snapshot.status_bar_appears(),
    );

    second_client.quit();
    zellij.quit();
}
