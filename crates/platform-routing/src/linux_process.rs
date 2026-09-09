//! Linux-only process lifetime boundary. Runs on the route owner, never an audio callback.

use std::{io, os::unix::process::CommandExt, process::Command};

pub(super) fn bind_to_parent(command: &mut Command) {
    let parent = std::process::id();
    // SAFETY: After fork the hook only invokes prctl/getppid and constructs raw OS errors.
    // It captures a PID by value and never allocates, locks, logs or accesses the environment.
    // All prctl variadic arguments use the ABI's unsigned-long width. No pointers are passed.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(
                libc::PR_SET_PDEATHSIG,
                libc::SIGKILL as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
            ) == -1
            {
                return Err(io::Error::last_os_error());
            }
            // The parent could have died between fork and prctl. In that case no death
            // signal is pending: fail before exec instead of leaving an orphan client.
            if libc::getppid() as u32 != parent {
                return Err(io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        });
    }
}
