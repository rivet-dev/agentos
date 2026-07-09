#define _GNU_SOURCE

#include <errno.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

static void put_u32(unsigned char *buffer, size_t *offset, uint32_t value) {
    buffer[(*offset)++] = (unsigned char)(value >> 24);
    buffer[(*offset)++] = (unsigned char)(value >> 16);
    buffer[(*offset)++] = (unsigned char)(value >> 8);
    buffer[(*offset)++] = (unsigned char)value;
}

static uint32_t get_u32(const unsigned char *buffer) {
    return ((uint32_t)buffer[0] << 24) | ((uint32_t)buffer[1] << 16) |
        ((uint32_t)buffer[2] << 8) | buffer[3];
}

static void put_string(unsigned char *buffer, size_t *offset,
    const char *value) {
    size_t length = strlen(value);
    put_u32(buffer, offset, (uint32_t)length);
    memcpy(buffer + *offset, value, length);
    *offset += length;
}

static int write_all(int fd, const void *data, size_t length) {
    const unsigned char *cursor = data;
    while (length != 0) {
        ssize_t written = write(fd, cursor, length);
        if (written < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        cursor += written;
        length -= (size_t)written;
    }
    return 0;
}

static int read_all(int fd, void *data, size_t length) {
    unsigned char *cursor = data;
    while (length != 0) {
        ssize_t received = read(fd, cursor, length);
        if (received <= 0) {
            if (received < 0 && errno == EINTR) continue;
            return -1;
        }
        cursor += received;
        length -= (size_t)received;
    }
    return 0;
}

int main(void) {
    enum { SSH_SK_HELPER_VERSION = 5, SSH_SK_HELPER_ERROR = 0,
        SSH_SK_HELPER_ENROLL = 2, KEY_ED25519_SK = 8 };
    unsigned char request[512], response[512], header[4];
    int input[2], output[2], status = 0, action_error;
    posix_spawn_file_actions_t actions;
    pid_t child;
    size_t offset = 4;
    char *argv[] = {(char *)"ssh-sk-helper", NULL};

    request[offset++] = SSH_SK_HELPER_VERSION;
    put_u32(request, &offset, SSH_SK_HELPER_ENROLL);
    request[offset++] = 1; /* helper errors to stderr */
    put_u32(request, &offset, 3); /* SYSLOG_LEVEL_ERROR */
    put_u32(request, &offset, KEY_ED25519_SK);
    put_string(request, &offset, "internal");
    put_string(request, &offset, "");
    put_string(request, &offset, "ssh:agentos-test");
    put_string(request, &offset, "agentos");
    request[offset++] = 0;
    put_string(request, &offset, "");
    put_string(request, &offset, "");
    {
        size_t header_offset = 0;
        put_u32(request, &header_offset, (uint32_t)(offset - 4));
    }

    if (pipe(input) != 0 || pipe(output) != 0 ||
        posix_spawn_file_actions_init(&actions) != 0)
        return 1;
    action_error = posix_spawn_file_actions_adddup2(&actions, input[0], 0);
    if (action_error == 0)
        action_error = posix_spawn_file_actions_adddup2(&actions, output[1], 1);
    if (action_error == 0)
        action_error = posix_spawn_file_actions_addclose(&actions, input[1]);
    if (action_error == 0)
        action_error = posix_spawn_file_actions_addclose(&actions, output[0]);
    if (action_error != 0)
        return 1;
    action_error = posix_spawnp(&child, argv[0], &actions, NULL, argv, environ);
    posix_spawn_file_actions_destroy(&actions);
    close(input[0]);
    close(output[1]);
    if (action_error != 0)
        return 1;
    if (write_all(input[1], request, offset) != 0)
        return 1;
    close(input[1]);
    if (read_all(output[0], header, sizeof(header)) != 0)
        return 1;
    uint32_t response_length = get_u32(header);
    if (response_length > sizeof(response) ||
        read_all(output[0], response, response_length) != 0)
        return 1;
    close(output[0]);
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status) ||
        WEXITSTATUS(status) != 0)
        return 1;

    int ok = response_length >= 9 && response[0] == SSH_SK_HELPER_VERSION &&
        get_u32(response + 1) == SSH_SK_HELPER_ERROR &&
        get_u32(response + 5) != 0;
    printf("ssh_sk_helper_framed_provider_error=%s\n", ok ? "yes" : "no");
    return ok ? 0 : 1;
}
