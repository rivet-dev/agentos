/**
 * Process-wide writeback barrier for AgentOS filesystems.
 *
 * AgentOS VFS mutations await block-store writes and metadata commits before
 * returning to the guest, so there is no deferred kernel writeback queue for
 * sync() to drain. Reaching this function is therefore the required barrier.
 */
#include <unistd.h>

void sync(void) {}

int syncfs(int fd) {
    return fsync(fd);
}
