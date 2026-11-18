/* Isolated Darwin API fixture. This is not the CCCC session owner. */
#ifndef __APPLE__
#error This fixture requires the macOS SDK and must run on macOS.
#endif

#include <errno.h>
#include <inttypes.h>
#include <libproc.h>
#include <limits.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/proc_info.h>
#include <sys/sysctl.h>
#include <unistd.h>

#ifndef PROBE_BUILD
#define PROBE_BUILD 1
#endif

static int failure(const char *stage, int error) {
    printf("{\"ok\":false,\"stage\":\"%s\",\"errno\":%d}\n", stage, error);
    fflush(stdout);
    return 1;
}

static int boot_uuid(char *buffer, size_t capacity) {
    size_t size = capacity;
    memset(buffer, 0, capacity);
    if (sysctlbyname("kern.bootsessionuuid", buffer, &size, NULL, 0) != 0) {
        return failure("boot_uuid", errno);
    }
    if (size > capacity || strnlen(buffer, capacity) != 36) {
        return failure("boot_uuid_shape", EINVAL);
    }
    for (size_t i = 0; i < 36; ++i) {
        char c = buffer[i];
        if (i == 8 || i == 13 || i == 18 || i == 23) {
            if (c != '-') {
                return failure("boot_uuid_separator", EINVAL);
            }
        } else if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') ||
                     (c >= 'A' && c <= 'F'))) {
            return failure("boot_uuid_digit", EINVAL);
        }
    }
    return 0;
}

static int identity(pid_t pid) {
    struct proc_bsdinfo info;
    memset(&info, 0, sizeof(info));
    errno = 0;
    int bytes = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, sizeof(info));
    if (bytes != (int)sizeof(info)) {
        int error = errno;
        printf("{\"ok\":false,\"stage\":\"bsd_info\",\"pid\":%d,"
               "\"errno\":%d,\"returned_bytes\":%d}\n", pid, error, bytes);
        fflush(stdout);
        return 1;
    }
    char boot[64];
    if (boot_uuid(boot, sizeof(boot)) != 0) {
        return 1;
    }
    printf("{\"ok\":true,\"build\":%d,\"pid\":%u,\"ppid\":%u,"
           "\"uid\":%u,\"gid\":%u,\"pgid\":%u,\"sid\":%d,"
           "\"birth_seconds\":%" PRIu64 ",\"birth_microseconds\":%" PRIu64 ","
           "\"bsd_info_bytes\":%d,\"boot_uuid\":\"%s\"}\n",
           PROBE_BUILD, info.pbi_pid, info.pbi_ppid, info.pbi_uid, info.pbi_gid,
           info.pbi_pgid, getsid(pid), (uint64_t)info.pbi_start_tvsec,
           (uint64_t)info.pbi_start_tvusec, bytes, boot);
    fflush(stdout);
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--inspect") == 0) {
        char *end;
        errno = 0;
        long pid = strtol(argv[2], &end, 10);
        if (errno != 0 || *end != '\0' || pid <= 0 || pid > INT_MAX) {
            return failure("invalid_pid", EINVAL);
        }
        return identity((pid_t)pid);
    }
    return failure("invalid_arguments", EINVAL);
}
