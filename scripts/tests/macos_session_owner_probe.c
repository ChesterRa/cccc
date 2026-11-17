/* Isolated Darwin API fixture. This is not the CCCC session owner. */
#ifndef __APPLE__
#error This fixture requires the macOS SDK and must run on macOS.
#endif

#include <errno.h>
#include <inttypes.h>
#include <libproc.h>
#include <limits.h>
#include <spawn.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/proc_info.h>
#include <sys/sysctl.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

#ifndef PROBE_BUILD
#define PROBE_BUILD 1
#endif

extern char **environ;

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

static int serve(void) {
    if (identity(getpid()) != 0) {
        return 1;
    }
    char command[8192];
    while (fgets(command, sizeof(command), stdin) != NULL) {
        size_t size = strlen(command);
        if (size == 0 || command[size - 1] != '\n') {
            return failure("command_too_long", EINVAL);
        }
        command[size - 1] = '\0';
        if (strcmp(command, "identity") == 0) {
            if (identity(getpid()) != 0) {
                return 1;
            }
        } else if (strcmp(command, "quit") == 0) {
            return 0;
        } else if (strncmp(command, "exec /", 6) == 0) {
            const char *path = command + 5;
            execl(path, path, "--serve", (char *)NULL);
            return failure("exec", errno);
        } else if (strncmp(command, "spawn /", 7) == 0) {
            char *path = command + 6;
            char *arguments[] = {path, "--identity", NULL};
            pid_t child;
            int error = posix_spawn(&child, path, NULL, NULL, arguments, environ);
            if (error != 0) {
                return failure("spawn", error);
            }
            int status;
            while (waitpid(child, &status, 0) < 0) {
                if (errno != EINTR) {
                    return failure("wait_child", errno);
                }
            }
            printf("{\"ok\":%s,\"stage\":\"spawn_complete\",\"child_pid\":%d,"
                   "\"status\":%d}\n",
                   WIFEXITED(status) && WEXITSTATUS(status) == 0 ? "true" : "false",
                   child, WIFEXITED(status) ? WEXITSTATUS(status) : -1);
            fflush(stdout);
        } else {
            return failure("unknown_command", EINVAL);
        }
    }
    return 0;
}

static int peer_server(const char *path) {
    struct sockaddr_un address;
    memset(&address, 0, sizeof(address));
    size_t size = strlen(path);
    if (size >= sizeof(address.sun_path)) {
        return failure("socket_path_too_long", ENAMETOOLONG);
    }
    address.sun_family = AF_UNIX;
    address.sun_len = (unsigned char)(offsetof(struct sockaddr_un, sun_path) + size + 1);
    memcpy(address.sun_path, path, size + 1);
    int listener = socket(AF_UNIX, SOCK_STREAM, 0);
    if (listener < 0) {
        return failure("socket", errno);
    }
    if (bind(listener, (struct sockaddr *)&address, address.sun_len) != 0 ||
        chmod(path, 0600) != 0 || listen(listener, 1) != 0) {
        int error = errno;
        close(listener);
        return failure("bind_listen", error);
    }
    printf("{\"ok\":true,\"stage\":\"peer_ready\",\"pid\":%d}\n", getpid());
    fflush(stdout);
    int peer = accept(listener, NULL, NULL);
    if (peer < 0) {
        int error = errno;
        close(listener);
        return failure("accept", error);
    }
    uid_t uid;
    gid_t gid;
    pid_t pid;
    socklen_t pid_size = sizeof(pid);
    int result = 0;
    if (getpeereid(peer, &uid, &gid) != 0 ||
        getsockopt(peer, SOL_LOCAL, LOCAL_PEERPID, &pid, &pid_size) != 0 ||
        pid_size != sizeof(pid)) {
        result = failure("peer_credentials", errno);
    } else {
        char token;
        if (read(peer, &token, 1) != 1 || token != 'x') {
            result = failure("peer_token", EINVAL);
        } else if (write(peer, "ok\n", 3) != 3) {
            result = failure("peer_reply", errno);
        } else {
            printf("{\"ok\":true,\"stage\":\"peer_credentials\",\"uid\":%u,"
                   "\"gid\":%u,\"pid\":%d}\n", uid, gid, pid);
            fflush(stdout);
        }
    }
    close(peer);
    close(listener);
    return result;
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--identity") == 0) {
        return identity(getpid());
    }
    if (argc == 2 && strcmp(argv[1], "--serve") == 0) {
        return serve();
    }
    if (argc == 3 && strcmp(argv[1], "--peer") == 0) {
        return peer_server(argv[2]);
    }
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
