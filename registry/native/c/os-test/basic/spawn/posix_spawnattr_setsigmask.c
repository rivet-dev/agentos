/*[SPN]*/
/* Test whether a basic posix_spawnattr_setsigmask invocation works. */

#include <sys/wait.h>

#include <spawn.h>
#include <signal.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

extern char** environ;

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "child invoked incorrectly");
		sigset_t set;
		if ( sigprocmask(SIG_SETMASK, NULL, &set) < 0 )
			err(1, "sigprocmask");
		if ( !sigismember(&set, SIGUSR1) )
			errx(1, "SIGUSR1 was not blocked");
		if ( sigismember(&set, SIGUSR2) )
			errx(1, "SIGUSR2 was blocked");
		return 0;
	}
	// Test setting a signal mask on spawn.
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	if ( (errno = posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSIGMASK)) )
		err(1, "posix_spawnattr_setflags");
	sigset_t set;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	if ( (errno = posix_spawnattr_setsigmask(&attr, &set)) )
		err(1, "posix_spawnattr_setsigmask");
	sigemptyset(&set);
	sigaddset(&set, SIGUSR2);
	if ( sigprocmask(SIG_BLOCK, &set, NULL) < 0 )
		err(1, "sigprocmask");
	char* new_argv[] =
	{
		argv[0],
		"success",
		NULL,
	};
	pid_t pid;
	if ( (errno = posix_spawn(&pid, argv[0], NULL, &attr, new_argv, environ)) )
		err(1, "posix_spawn: %s", new_argv[0]);
	int status;
	if ( waitpid(pid, &status, 0) < 0 )
		err(1, "waitpid");
	if ( WIFEXITED(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else
		errx(1, "unknown exit: %#x", status);
	return 0;
}
