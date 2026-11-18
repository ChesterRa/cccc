//! The file this process executes, as the kernel maps it. A build or an update
//! may since have put another file at the launch path; a lookup of that path,
//! or of the image's build UUID, would not tell the two apart.

use std::io;
use std::path::Path;
use std::time::SystemTime;

/// What `stat` reports for an executable file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Serialize, serde::Deserialize))]
pub struct ImageFile {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub written: SystemTime,
}

impl ImageFile {
    /// The regular file at `path` now, following links.
    pub fn at(path: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(path)?;
            if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the executable is not a regular file",
                ));
            }
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
                size: metadata.len(),
                written: metadata.modified()?,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "local executable file identity is unavailable on this platform",
            ))
        }
    }

    /// The image this process is executing, even when its launch path now
    /// names another file. Elsewhere this needs native image evidence; reading
    /// the launch path is not an equivalent fallback.
    pub fn running() -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        return Self::at(Path::new("/proc/self/exe"));
        #[cfg(target_os = "macos")]
        return darwin::running();
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "running executable identity is not implemented on this platform",
        ))
    }
}

/// The kernel reports the file behind the mapping of this process's code,
/// which stays the launched file after its path names another one.
#[cfg(target_os = "macos")]
mod darwin {
    use super::ImageFile;
    use libproc::proc_pid::{PIDInfo, PidInfoFlavor, pidinfo};
    use nix::libc::{c_int, vnode_info_path};
    use std::io;
    use std::time::{Duration, SystemTime};

    /// `struct proc_regioninfo` in xnu `bsd/sys/proc_info.h`. The kernel fills
    /// it through `pidinfo`; only the mapping's bounds are read.
    #[allow(dead_code)]
    #[repr(C)]
    struct RegionInfo {
        pri_protection: u32,
        pri_max_protection: u32,
        pri_inheritance: u32,
        pri_flags: u32,
        pri_offset: u64,
        pri_behavior: u32,
        pri_user_wired_count: u32,
        pri_user_tag: u32,
        pri_pages_resident: u32,
        pri_pages_shared_now_private: u32,
        pri_pages_swapped_out: u32,
        pri_pages_dirtied: u32,
        pri_ref_count: u32,
        pri_shadow_depth: u32,
        pri_share_mode: u32,
        pri_private_pages_resident: u32,
        pri_shared_pages_resident: u32,
        pri_obj_id: u32,
        pri_depth: u32,
        pri_address: u64,
        pri_size: u64,
    }

    /// `struct proc_regionwithpathinfo`, the `PROC_PIDREGIONPATHINFO` reply.
    #[allow(dead_code)]
    #[repr(C)]
    struct RegionWithPathInfo {
        prp_prinfo: RegionInfo,
        prp_vip: vnode_info_path,
    }

    impl PIDInfo for RegionWithPathInfo {
        fn flavor() -> PidInfoFlavor {
            PidInfoFlavor::RegionPathInfo
        }
    }

    /// `VREG` in xnu's `enum vtype`.
    const VREG: c_int = 1;

    pub(super) fn running() -> io::Result<ImageFile> {
        // This crate is linked into the executable, so this function's own
        // code lies in the executable's text mapping.
        let address = running as fn() -> io::Result<ImageFile> as usize as u64;
        let pid = i32::try_from(std::process::id()).map_err(io::Error::other)?;
        let info = pidinfo::<RegionWithPathInfo>(pid, address).map_err(|error| {
            io::Error::other(format!("read the running image mapping: {error}"))
        })?;
        // The kernel returns the next mapping when none contains the address,
        // and a zeroed vnode for an anonymous one; neither identifies a file.
        let region = &info.prp_prinfo;
        if address < region.pri_address || address - region.pri_address >= region.pri_size {
            return Err(invalid(
                "the reported mapping does not contain this process's code",
            ));
        }
        let vnode = &info.prp_vip.vip_vi;
        if vnode.vi_type != VREG || vnode.vi_stat.vst_ino == 0 {
            return Err(invalid(
                "this process's code is not mapped from a regular file",
            ));
        }
        let stat = &vnode.vi_stat;
        Ok(ImageFile {
            // `std::fs::Metadata::dev` widens the signed `dev_t`; match it.
            device: stat.vst_dev as i32 as u64,
            inode: stat.vst_ino,
            size: u64::try_from(stat.vst_size).map_err(|_| invalid("negative file size"))?,
            written: modified(stat.vst_mtime, stat.vst_mtimensec)?,
        })
    }

    fn modified(seconds: i64, nanoseconds: i64) -> io::Result<SystemTime> {
        let seconds = u64::try_from(seconds)
            .map_err(|_| invalid("the running image was modified before 1970"))?;
        let nanoseconds = u32::try_from(nanoseconds)
            .ok()
            .filter(|value| *value < 1_000_000_000)
            .ok_or_else(|| invalid("invalid modification time"))?;
        SystemTime::UNIX_EPOCH
            .checked_add(Duration::new(seconds, nanoseconds))
            .ok_or_else(|| invalid("modification time out of range"))
    }

    fn invalid(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
    }

    /// `pidinfo` hands the kernel this size and checks only that the reply is
    /// positive. These numbers are `<sys/proc_info.h>`'s sizeof and offsetof.
    #[cfg(test)]
    #[test]
    fn region_reply_matches_the_published_layout() {
        use std::mem::{offset_of, size_of};
        assert_eq!(size_of::<RegionInfo>(), 96);
        assert_eq!(offset_of!(RegionInfo, pri_offset), 16);
        assert_eq!(offset_of!(RegionInfo, pri_address), 80);
        assert_eq!(offset_of!(RegionInfo, pri_size), 88);
        assert_eq!(size_of::<vnode_info_path>(), 1176);
        assert_eq!(offset_of!(RegionWithPathInfo, prp_vip), 96);
        assert_eq!(size_of::<RegionWithPathInfo>(), 1272);
    }
}

#[cfg(test)]
mod tests {
    use super::ImageFile;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn running_image_keeps_its_identity_after_its_launch_path_is_replaced() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        const GATE: &str = "CCCC_TEST_RUNNING_IMAGE_GATE";
        const NAME: &str = "running_image::tests::running_image_keeps_its_identity_after_its_launch_path_is_replaced";
        if let Some(gate) = std::env::var_os(GATE) {
            // The copied image: report once the parent has replaced our path.
            let gate = std::path::PathBuf::from(gate);
            let deadline = Instant::now() + Duration::from_secs(20);
            while !gate.exists() {
                assert!(
                    Instant::now() < deadline,
                    "the launch path was not replaced"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let running = ImageFile::running().expect("running image");
            println!(
                "RUNNING_IMAGE {}",
                serde_json::to_string(&running).expect("identity")
            );
            return;
        }
        let temp = tempfile::tempdir().expect("isolated files");
        let image = temp.path().join("image");
        std::fs::copy(std::env::current_exe().expect("test image"), &image).expect("copy");
        let launched = ImageFile::at(&image).expect("launched identity");
        let gate = temp.path().join("replaced");
        // spawn returns after exec, so the copy is already the child's image.
        let child = Command::new(&image)
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(GATE, &gate)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run the copied image");
        let replacement = temp.path().join("next");
        std::fs::write(&replacement, b"not the running image").expect("replacement");
        std::fs::rename(&replacement, &image).expect("replace as a build does");
        std::fs::write(&gate, b"").expect("gate");
        let output = child.wait_with_output().expect("copied image result");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reported: ImageFile = stdout
            .lines()
            .find_map(|line| line.split_once("RUNNING_IMAGE ").map(|(_, json)| json))
            .and_then(|json| serde_json::from_str(json).ok())
            .expect("reported identity");
        assert_eq!(reported, launched);
        assert_ne!(reported, ImageFile::at(&image).expect("replacement"));
    }
}
