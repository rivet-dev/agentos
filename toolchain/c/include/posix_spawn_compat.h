/* posix_spawn_compat.h -- shared posix_spawn types and declarations for WASI
 *
 * wasi-libc does not ship spawn.h or sys/wait.h, so programs that use
 * posix_spawn need forward declarations.  This header consolidates them
 * in one place so every C test program can #include it instead of
 * duplicating the typedefs inline.
 *
 * On non-WASI targets the real <spawn.h>, <sys/wait.h>, and <signal.h>
 * are included instead.
 */
#ifndef POSIX_SPAWN_COMPAT_H
#define POSIX_SPAWN_COMPAT_H

#ifdef __wasi__

#include <unistd.h>  /* pid_t */

/* --- Types ------------------------------------------------------------ */

/* Layout MUST match the patched wasi-libc posix_spawn implementation. */
typedef struct { int __pad0[2]; void *__actions; int __pad[16]; } posix_spawn_file_actions_t;
typedef struct { int __dummy; } posix_spawnattr_t;

/* --- posix_spawn ------------------------------------------------------ */

int posix_spawnp(pid_t *restrict, const char *restrict,
    const posix_spawn_file_actions_t *,
    const posix_spawnattr_t *restrict,
    char *const[restrict], char *const[restrict]);

/* --- file_actions API ------------------------------------------------- */

int posix_spawn_file_actions_init(posix_spawn_file_actions_t *);
int posix_spawn_file_actions_destroy(posix_spawn_file_actions_t *);
int posix_spawn_file_actions_adddup2(posix_spawn_file_actions_t *, int, int);
int posix_spawn_file_actions_addclose(posix_spawn_file_actions_t *, int);

/* --- waitpid / wait --------------------------------------------------- */

pid_t waitpid(pid_t, int *, int);
pid_t wait(int *);

/* --- wait status macros ----------------------------------------------- */

#define WEXITSTATUS(s) (((s) & 0xff00) >> 8)
#define WIFEXITED(s)   (!((s) & 0x7f))
#define WIFSIGNALED(s) (((s) & 0x7f) != 0 && ((s) & 0x7f) != 0x7f)
#define WTERMSIG(s)    ((s) & 0x7f)

/* --- signals ---------------------------------------------------------- */

int kill(pid_t, int);

#ifndef SIGKILL
#define SIGKILL 9
#endif
#ifndef SIGTERM
#define SIGTERM 15
#endif

#else /* !__wasi__ */

#include <spawn.h>
#include <sys/wait.h>
#include <signal.h>

#endif /* __wasi__ */

#endif /* POSIX_SPAWN_COMPAT_H */
