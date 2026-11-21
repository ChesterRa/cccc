//! Short local socket paths without moving their durable owner directories.

#[cfg(target_os = "linux")]
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// Keep this value alive until bind/connect has resolved its path. Established
/// connections no longer need the directory descriptor used by the Linux alias.
pub struct SocketPath {
    pub path: PathBuf,
    #[cfg(target_os = "linux")]
    _directory: File,
    #[cfg(target_os = "macos")]
    _alias: Option<tempfile::TempDir>,
}

impl SocketPath {
    pub fn new(socket: &Path) -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let parent = socket.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "socket has no parent directory",
                )
            })?;
            let name = socket.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "socket has no file name")
            })?;
            let directory = File::open(parent)?;
            let path = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())).join(name);
            Ok(Self {
                path,
                _directory: directory,
            })
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;
            use std::os::unix::fs::PermissionsExt;
            // Darwin's sockaddr_un.sun_path has 104 bytes, including NUL.
            if socket.as_os_str().as_bytes().len() < 104 {
                return Ok(Self {
                    path: socket.to_path_buf(),
                    _alias: None,
                });
            }
            let parent = socket.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "socket has no parent directory",
                )
            })?;
            let name = socket.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "socket has no file name")
            })?;
            let parent = std::fs::canonicalize(parent)?;
            // A fresh private directory prevents other users from replacing
            // the alias. /tmp stays short even under a long TMPDIR.
            let directory = tempfile::Builder::new()
                .prefix(".cccc-sock-")
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir_in("/tmp")?;
            let alias = directory.path().join("d");
            std::os::unix::fs::symlink(parent, &alias)?;
            let path = alias.join(name);
            if path.as_os_str().as_bytes().len() >= 104 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the socket file name is too long for a short Darwin alias",
                ));
            }
            Ok(Self {
                path,
                _alias: Some(directory),
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        Ok(Self {
            path: socket.to_path_buf(),
        })
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};

    #[test]
    fn a_long_owner_path_connects_through_independent_short_aliases() {
        let temporary = tempfile::tempdir().expect("isolated socket home");
        let directory = temporary.path().join("x".repeat(90)).join("y".repeat(90));
        std::fs::create_dir_all(&directory).expect("long owner directory");
        let durable = directory.join("control.sock");
        let listener = {
            let alias = SocketPath::new(&durable).expect("bind alias");
            UnixListener::bind(&alias.path).expect("bind through alias")
        };
        assert!(
            durable.exists(),
            "the socket remains in its owner directory"
        );
        let mut client = {
            let alias = SocketPath::new(&durable).expect("independent connect alias");
            UnixStream::connect(&alias.path).expect("connect through another alias")
        };
        let (mut server, _) = listener.accept().expect("accept");
        client
            .write_all(b"ok")
            .expect("write after both aliases dropped");
        let mut bytes = [0; 2];
        server.read_exact(&mut bytes).expect("read");
        assert_eq!(&bytes, b"ok");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_darwin_alias_is_private_and_removed_without_removing_its_target() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let temporary = tempfile::tempdir().expect("isolated socket home");
        let directory = temporary.path().join("x".repeat(90)).join("y".repeat(90));
        std::fs::create_dir_all(&directory).expect("long owner directory");
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o750))
            .expect("distinct owner directory mode");
        let alias = SocketPath::new(&directory.join("control.sock")).expect("alias");
        let private = alias
            .path
            .parent()
            .expect("d")
            .parent()
            .expect("private directory")
            .to_path_buf();
        assert_eq!(
            std::fs::metadata(&private).expect("private mode").mode() & 0o777,
            0o700
        );
        drop(alias);
        assert!(!private.exists());
        let target = std::fs::metadata(directory).expect("durable owner directory");
        assert!(target.is_dir());
        assert_eq!(target.mode() & 0o777, 0o750);
    }
}
