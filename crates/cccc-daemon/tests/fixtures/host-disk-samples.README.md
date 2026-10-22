# Captured filesystem samples

Three real Linux filesystem samples captured with `os.statvfs` on
2026-10-22. Filesystem paths are omitted and the records use aliases.
All timestamps, block counts and byte counts are unchanged.
`available_bytes = f_bavail * f_frsize`; `total_bytes = f_blocks * f_frsize`.

These are policy inputs, not a claimed history of one volume changing size.
The low/recovery tests replay them to exercise hysteresis without filling a
filesystem or requiring spare capacity. Exact-boundary tests place policy
thresholds at captured values, rather than manufacturing capacity samples.
The independent filesystem probe uses a small temporary directory and a
missing child path; it asserts no free-space floor.
