/*[SPN]*/
/* Test whether a basic posix_spawn_file_actions_init invocation works. */

#include <sys/wait.h>

#include <spawn.h>
#include <signal.h>
#include <string.h>

#include "../basic.h"

extern char** environ;

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "child invoked incorrectly");
		return 0;
	}
	posix_spawn_file_actions_t actions;
	if ( (errno = posix_spawn_file_actions_init(&actions)) )
		err(1, "posix_spawn_file_actions_init");
	char* new_argv[] =
	{
		argv[0],
		"success",
		NULL,
	};
	pid_t pid;
	if ( (errno = posix_spawn(&pid, argv[0], &actions, NULL, new_argv,
	                          environ)) )
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
