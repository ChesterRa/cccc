from __future__ import annotations

from scripts.verify_standalone_binary import (
    parse_elf_needed,
    parse_elf_version_references,
    parse_macos_dependencies,
    parse_macos_minimum_versions,
    validate_linux,
    validate_macos,
)


def test_linux_standalone_accepts_the_manylinux_2_28_system_boundary() -> None:
    versions = """
      0x0010:   Name: GLIBC_2.28  Flags: none  Version: 11
      0x0020:   Name: GLIBCXX_3.4.24  Flags: none  Version: 10
      0x0030:   Name: CXXABI_1.3.11  Flags: none  Version: 9
      0x0040:   Name: GCC_7.0.0  Flags: none  Version: 8
    """
    dynamic = """
      0x0000000000000001 (NEEDED) Shared library: [libstdc++.so.6]
      0x0000000000000001 (NEEDED) Shared library: [libgcc_s.so.1]
      0x0000000000000001 (NEEDED) Shared library: [libm.so.6]
      0x0000000000000001 (NEEDED) Shared library: [libc.so.6]
      0x0000000000000001 (NEEDED) Shared library: [ld-linux-x86-64.so.2]
    """

    references = parse_elf_version_references(versions)
    needed = parse_elf_needed(dynamic)

    assert references["GLIBC"] == {(2, 28)}
    assert needed == {
        "ld-linux-x86-64.so.2",
        "libc.so.6",
        "libgcc_s.so.1",
        "libm.so.6",
        "libstdc++.so.6",
    }
    assert validate_linux(versions, dynamic) == []


def test_linux_standalone_rejects_newer_abi_and_runtime_openssl() -> None:
    versions = """
      Name: GLIBC_2.39
      Name: GLIBCXX_3.4.30
      Name: CXXABI_1.3.13
    """
    dynamic = """
      (NEEDED) Shared library: [libc.so.6]
      (NEEDED) Shared library: [libssl.so.3]
      (NEEDED) Shared library: [libcrypto.so.3]
    """

    errors = validate_linux(versions, dynamic)

    assert any("GLIBC_2.39" in error for error in errors)
    assert any("GLIBCXX_3.4.30" in error for error in errors)
    assert any("CXXABI_1.3.13" in error for error in errors)
    assert any("libssl.so.3" in error and "libcrypto.so.3" in error for error in errors)


def test_macos_standalone_accepts_11_0_and_system_dependencies() -> None:
    build = """
    Load command 10
          cmd LC_BUILD_VERSION
     platform MACOS
        minos 11.0
          sdk 15.4
       ntools 1
         tool LD
      version 1053.12
    """
    libraries = """
    cccc:
        /usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1345.120.2)
        /System/Library/Frameworks/Security.framework/Versions/A/Security (compatibility version 1.0.0, current version 61439.120.27)
    """

    assert parse_macos_minimum_versions(build) == [(11, 0)]
    assert parse_macos_dependencies(libraries) == {
        "/System/Library/Frameworks/Security.framework/Versions/A/Security",
        "/usr/lib/libSystem.B.dylib",
    }
    assert validate_macos(build, libraries) == []


def test_macos_standalone_reads_legacy_version_min_command() -> None:
    build = """
    Load command 9
          cmd LC_VERSION_MIN_MACOSX
      cmdsize 16
      version 10.15
          sdk 11.3
    """

    assert parse_macos_minimum_versions(build) == [(10, 15)]


def test_macos_standalone_rejects_newer_baseline_and_non_system_dylib() -> None:
    build = """
    Load command 10
          cmd LC_BUILD_VERSION
      cmdsize 32
     platform MACOS
        minos 13.0
          sdk 15.4
    """
    libraries = """
    cccc:
        @rpath/libssl.3.dylib (compatibility version 3.0.0, current version 3.5.0)
        /usr/local/lib/libcustom.dylib (compatibility version 1.0.0, current version 1.0.0)
    """

    errors = validate_macos(build, libraries)

    assert any("13.0" in error for error in errors)
    assert any("@rpath/libssl.3.dylib" in error for error in errors)
    assert any("/usr/local/lib/libcustom.dylib" in error for error in errors)
