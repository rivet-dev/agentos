/*[SPN]*/
/* Test whether a basic posix_spawnattr_setpgroup invocation works. */

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
		if ( getpgid(0) != getpid() )
			errx(1, "child did not have its own process group");
		return 0;
	}
	// Test putting the child in its own process group.
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	if ( (errno = posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETPGROUP)) )
		err(1, "posix_spawnattr_setflags");
	if ( (errno = posix_spawnattr_setpgroup(&attr, 0)) )
		err(1, "posix_spawnattr_setpgroup");
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
