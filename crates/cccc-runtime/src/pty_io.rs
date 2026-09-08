//! Linux PTY IO must observe hangup even when the slave stops consuming input.
//! Owned descriptors keep poll identities stable, without unsafe raw-FD ownership.
use filedescriptor::{FileDescriptor, POLLERR, POLLHUP, POLLIN, POLLOUT, poll, pollfd};
use portable_pty::MasterPty;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;

pub(crate) fn open(master: &dyn MasterPty) -> io::Result<(PtyIo, PtyIo)> {
    let raw = master
        .as_raw_fd()
        .ok_or_else(|| io::Error::other("PTY has no Unix descriptor"))?;
    let mut reader = FileDescriptor::dup(&raw).map_err(io::Error::other)?;
    // Duplicates share the file description, so both directions use readiness
    // waits. Applying O_NONBLOCK only to writes would also affect the reader.
    reader.set_non_blocking(true).map_err(io::Error::other)?;
    let writer = reader.try_clone().map_err(io::Error::other)?;
    Ok((PtyIo(reader), PtyIo(writer)))
}

pub(crate) struct PtyIo(FileDescriptor);

impl PtyIo {
    fn wait(&self, events: i16, timeout: Option<std::time::Duration>) -> io::Result<i16> {
        let mut descriptors = [pollfd {
            fd: self.0.as_raw_fd(),
            events,
            revents: 0,
        }];
        loop {
            match poll(&mut descriptors, timeout) {
                Ok(_) => return Ok(descriptors[0].revents),
                Err(filedescriptor::Error::Poll(error))
                    if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(io::Error::other(error)),
            }
        }
    }
}

impl Read for PtyIo {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.0.read(bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    self.wait(POLLIN, None)?;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                // Unix PTYs report EIO once the slave has closed.
                Err(error) if error.raw_os_error() == Some(nix::libc::EIO) => return Ok(0),
                result => return result,
            }
        }
    }
}

impl Write for PtyIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            match self.0.write(bytes) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if self.wait(POLLOUT, None)? & (POLLHUP | POLLERR) != 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "PTY slave closed",
                        ));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl crate::pty_input::PtyInput for PtyIo {
    fn write_all_interruptible(
        &mut self,
        mut data: &[u8],
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<bool> {
        while !data.is_empty() {
            if cancelled() {
                return Ok(false);
            }
            match self.0.write(data) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(written) => data = &data[written..],
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if self.wait(POLLOUT, Some(std::time::Duration::from_millis(25)))?
                        & (POLLHUP | POLLERR)
                        != 0
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "PTY slave closed",
                        ));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        self.flush()?;
        Ok(true)
    }
}
