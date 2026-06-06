/*[SPN]*/
/* Test whether a basic posix_spawn invocation works. */

#include <sys/wait.h>

#include <spawn.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>

#include "../basic.h"

extern char** environ;

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "child invoked incorrectly");
		if ( !getenv("OS_TEST_POSIX_SPAWN") )
			errx(1, "$OS_TEST_POSIX_SPAWN unset");
		return 0;
	}
	// Test the environment is properly inherited.
	if ( setenv("OS_TEST_POSIX_SPAWN", "set", 1) < 0 )
		err(1, "setenv");
	const char* program = "spawn/posix_spawn";
	// posix_spawn does not search PATH
	char* new_argv[] =
	{
		"posix_spawn_child", // Does not exist, do not use.
		"success",
		NULL,
	};
	pid_t pid;
	if ( (errno = posix_spawn(&pid, program, NULL, NULL, new_argv,
	                          environ)) )
		err(1, "posix_spawn: %s", program);
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
