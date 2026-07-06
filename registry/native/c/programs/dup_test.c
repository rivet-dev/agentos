/* dup_test.c — duplicate stdout FD, write through duplicate */
#include <stdio.h>
#include <unistd.h>
#include <string.h>
#include <errno.h>
#include <sys/stat.h>
#include <fcntl.h>
#if defined(__wasi__)
#include <wasi/api.h>
#endif

int main(void) {
    int saved_stdout = dup(STDOUT_FILENO);
    if (saved_stdout < 0) {
        perror("dup saved stdout");
        return 1;
    }

    int saved_stderr = dup(STDERR_FILENO);
    if (saved_stderr < 0) {
        perror("dup saved stderr");
        return 1;
    }

    if (dup2(saved_stdout, saved_stdout) != saved_stdout) {
        perror("dup2 same stdout fd");
        return 1;
    }

    if (dup2(saved_stderr, saved_stderr) != saved_stderr) {
        perror("dup2 same stderr fd");
        return 1;
    }

#if defined(__wasi__)
    FILE *preopen_file = fopen("dup-preopen-check.txt", "w");
    if (!preopen_file) {
        perror("create preopen relative file");
        return 1;
    }
    fputs("preopen ok\n", preopen_file);
    fclose(preopen_file);

    int throwaway_preopen = dup(3);
    if (throwaway_preopen < 0) {
        perror("dup throwaway preopen");
        return 1;
    }

    if (close(throwaway_preopen) != 0) {
        perror("close throwaway preopen");
        return 1;
    }

    FILE *canonical_preopen_file = fopen("dup-preopen-check.txt", "r");
    if (!canonical_preopen_file) {
        perror("canonical preopen closed by duplicate close");
        return 1;
    }
    fclose(canonical_preopen_file);

    int saved_preopen = dup(3);
    if (saved_preopen < 0) {
        perror("dup preopen");
        return 1;
    }

    if (close(3) != 0) {
        perror("close preopen");
        return 1;
    }

	    errno = 0;
	    struct stat closed_preopen_stat;
	    if (fstat(3, &closed_preopen_stat) != -1 || errno != EBADF) {
	        dprintf(saved_stderr, "fstat resurrected closed preopen\n");
	        return 1;
	    }

	    __wasi_prestat_t closed_preopen_prestat;
	    __wasi_fdstat_t closed_preopen_fdstat;
	    uint8_t closed_preopen_name[16];
	    if (__wasi_fd_prestat_get((__wasi_fd_t)3, &closed_preopen_prestat) != __WASI_ERRNO_BADF) {
	        dprintf(saved_stderr, "prestat resurrected closed preopen\n");
	        return 1;
	    }
	    if (__wasi_fd_prestat_dir_name((__wasi_fd_t)3, closed_preopen_name, sizeof(closed_preopen_name)) != __WASI_ERRNO_BADF) {
	        dprintf(saved_stderr, "prestat dir name resurrected closed preopen\n");
	        return 1;
	    }
	    if (__wasi_fd_fdstat_get((__wasi_fd_t)3, &closed_preopen_fdstat) != __WASI_ERRNO_BADF) {
	        dprintf(saved_stderr, "fdstat resurrected closed preopen\n");
	        return 1;
	    }

	    if (fstat(saved_preopen, &closed_preopen_stat) != 0) {
	        dprintf(saved_stderr, "preopen duplicate closed too early\n");
	        return 1;
    }

    int pipefd[2];
    if (pipe(pipefd) != 0) {
        dprintf(saved_stderr, "pipe for preopen overlay failed\n");
        return 1;
    }

	    if (dup2(pipefd[0], 3) != 3) {
	        dprintf(saved_stderr, "dup2 pipe over preopen failed\n");
	        return 1;
	    }

	    if (__wasi_fd_prestat_get((__wasi_fd_t)3, &closed_preopen_prestat) != __WASI_ERRNO_BADF) {
	        dprintf(saved_stderr, "pipe overlay exposed preopen prestat\n");
	        return 1;
	    }
	    if (__wasi_fd_fdstat_get((__wasi_fd_t)3, &closed_preopen_fdstat) != __WASI_ERRNO_SUCCESS) {
	        dprintf(saved_stderr, "pipe overlay fdstat failed\n");
	        return 1;
	    }

	    int pipe_overlay_fd = openat(3, "dup-preopen-check.txt", O_RDONLY);
	    if (pipe_overlay_fd >= 0) {
	        close(pipe_overlay_fd);
	        dprintf(saved_stderr, "pipe overlay resurrected closed preopen\n");
        return 1;
    }

    close(pipefd[0]);
    close(pipefd[1]);

    if (dup2(saved_preopen, 3) != 3) {
        dprintf(saved_stderr, "dup2 failed to restore preopen\n");
        return 1;
    }

    int restored_preopen_fd = openat(3, "dup-preopen-check.txt", O_RDONLY);
    if (restored_preopen_fd < 0) {
        dprintf(saved_stderr, "restored preopen path_open failed\n");
        return 1;
    }
    close(restored_preopen_fd);

    if (close(saved_preopen) != 0) {
        dprintf(saved_stderr, "close preopen duplicate failed\n");
        return 1;
    }

    if (close(3) != 0) {
        dprintf(saved_stderr, "close restored preopen failed\n");
        return 1;
    }

    errno = 0;
	    if (fstat(3, &closed_preopen_stat) != -1 || errno != EBADF) {
	        dprintf(saved_stderr, "restored preopen resurrected after close\n");
	        return 1;
	    }
#endif

	    FILE *rewind_file = fopen("dup-rewind-clearerr.txt", "w+");
	    if (!rewind_file) {
	        perror("create rewind file");
	        return 1;
	    }
	    if (fputs("rewind-ok", rewind_file) < 0) {
	        perror("write rewind file");
	        fclose(rewind_file);
	        return 1;
	    }
	    fflush(rewind_file);
	    rewind(rewind_file);
	    while (fgetc(rewind_file) != EOF) {
	    }
	    if (!feof(rewind_file) || ferror(rewind_file)) {
	        dprintf(saved_stderr, "rewind file did not reach clean eof\n");
	        fclose(rewind_file);
	        return 1;
	    }
	    clearerr(rewind_file);
	    if (feof(rewind_file) || ferror(rewind_file)) {
	        dprintf(saved_stderr, "clearerr did not clear eof/error state\n");
	        fclose(rewind_file);
	        return 1;
	    }
	    rewind(rewind_file);
	    if (feof(rewind_file) || ferror(rewind_file) || fgetc(rewind_file) != 'r') {
	        dprintf(saved_stderr, "rewind did not reset stream state and position\n");
	        fclose(rewind_file);
	        return 1;
	    }
	    fclose(rewind_file);
	    unlink("dup-rewind-clearerr.txt");

	    /* Test dup: duplicate stdout */
	    int new_fd = dup(STDOUT_FILENO);
    if (new_fd < 0) {
        perror("dup");
        return 1;
    }

    const char *msg1 = "hello from dup\n";
    write(new_fd, msg1, strlen(msg1));
    close(new_fd);

    /* Test dup2: duplicate stdout to fd 10 */
    int fd2 = dup2(STDOUT_FILENO, 10);
    if (fd2 != 10) {
        fprintf(stderr, "dup2 returned %d, expected 10\n", fd2);
        return 1;
    }

    const char *msg2 = "hello from dup2\n";
    write(fd2, msg2, strlen(msg2));
    close(fd2);

    if (close(STDOUT_FILENO) != 0) {
        perror("close stdout");
        return 1;
    }

    errno = 0;
    const char *closed_stdout_msg = "closed stdout leak\n";
    if (write(STDOUT_FILENO, closed_stdout_msg, strlen(closed_stdout_msg)) != -1 || errno != EBADF) {
        dprintf(saved_stderr, "write to closed stdout did not fail with EBADF\n");
        return 1;
    }

    if (dup2(saved_stdout, STDOUT_FILENO) != STDOUT_FILENO) {
        dprintf(saved_stderr, "dup2 failed to restore stdout\n");
        return 1;
    }

    const char *restored_stdout_msg = "stdout restored\n";
    if (write(STDOUT_FILENO, restored_stdout_msg, strlen(restored_stdout_msg)) != (ssize_t)strlen(restored_stdout_msg)) {
        dprintf(saved_stderr, "write to restored stdout failed\n");
        return 1;
    }

    if (close(STDOUT_FILENO) != 0) {
        dprintf(saved_stderr, "close restored stdout failed\n");
        return 1;
    }

    errno = 0;
    if (write(STDOUT_FILENO, closed_stdout_msg, strlen(closed_stdout_msg)) != -1 || errno != EBADF) {
        dprintf(saved_stderr, "write resurrected closed stdout\n");
        return 1;
    }

    if (dup2(saved_stdout, STDOUT_FILENO) != STDOUT_FILENO) {
        dprintf(saved_stderr, "second dup2 failed to restore stdout\n");
        return 1;
    }

    if (close(STDERR_FILENO) != 0) {
        dprintf(saved_stdout, "close stderr failed\n");
        return 1;
    }

    errno = 0;
    const char *closed_stderr_msg = "closed stderr leak\n";
    if (write(STDERR_FILENO, closed_stderr_msg, strlen(closed_stderr_msg)) != -1 || errno != EBADF) {
        dprintf(saved_stdout, "write to closed stderr did not fail with EBADF\n");
        return 1;
    }

    if (dup2(saved_stderr, STDERR_FILENO) != STDERR_FILENO) {
        dprintf(saved_stdout, "dup2 failed to restore stderr\n");
        return 1;
    }

    const char *restored_stderr_msg = "stderr restored\n";
    if (write(STDERR_FILENO, restored_stderr_msg, strlen(restored_stderr_msg)) != (ssize_t)strlen(restored_stderr_msg)) {
        dprintf(saved_stdout, "write to restored stderr failed\n");
        return 1;
    }

    if (close(STDERR_FILENO) != 0) {
        dprintf(saved_stdout, "close restored stderr failed\n");
        return 1;
    }

    errno = 0;
    if (write(STDERR_FILENO, closed_stderr_msg, strlen(closed_stderr_msg)) != -1 || errno != EBADF) {
        dprintf(saved_stdout, "write resurrected closed stderr\n");
        return 1;
    }

    if (dup2(saved_stderr, STDERR_FILENO) != STDERR_FILENO) {
        dprintf(saved_stdout, "second dup2 failed to restore stderr\n");
        return 1;
    }

    close(saved_stdout);
    close(saved_stderr);

    /* Final output via original stdout */
    fflush(stdout);
    printf("done\n");
    return 0;
}
