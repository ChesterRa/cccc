use portable_pty::Child;

pub(crate) struct ProcessTreeGuard {
    #[cfg(unix)]
    process_group: Option<i32>,
    #[cfg(windows)]
    job: Option<win32job::Job>,
}

impl ProcessTreeGuard {
    pub(crate) fn attach(child: &dyn Child) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            let process_group = child
                .process_id()
                .and_then(|pid| i32::try_from(pid).ok())
                .ok_or_else(|| std::io::Error::other("PTY child has no valid process group"))?;
            Ok(Self {
                process_group: Some(process_group),
            })
        }

        #[cfg(windows)]
        {
            let handle = child
                .as_raw_handle()
                .ok_or_else(|| std::io::Error::other("PTY child has no process handle"))?;
            let mut limits = win32job::ExtendedLimitInfo::new();
            limits.limit_kill_on_job_close();
            let job = win32job::Job::create_with_limit_info(&limits)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            job.assign_process(handle as isize)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            Ok(Self { job: Some(job) })
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {})
        }
    }

    pub(crate) fn terminate(&mut self) {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group.take() {
            use nix::sys::signal::{Signal, killpg};
            use nix::unistd::Pid;

            let _ = killpg(Pid::from_raw(process_group), Signal::SIGKILL);
        }

        #[cfg(windows)]
        {
            // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE makes dropping the last handle
            // terminate the actor and all descendants in a single OS operation.
            self.job.take();
        }
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}
