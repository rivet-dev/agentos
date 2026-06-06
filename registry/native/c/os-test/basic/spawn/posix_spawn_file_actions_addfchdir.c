/*[SPN]*/
/* Test whether a basic posix_spawn_file_actions_addfchdir invocation works. */

#include <sys/wait.h>

#include <fcntl.h>
#include <spawn.h>
#include <signal.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

#if 0
#define posix_spawn_file_actions_addfchdir posix_spawn_file_actions_addfchdir_np
#endif

extern char** environ;

int main(int argc, char* argv[])
{
	char* program = "posix_spawn_file_actions_addfchdir";
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "child invoked incorrectly");
		// Test the program is in the current directory.
		if ( access(program, F_OK) < 0 )
			errx(1, "%s", program);
		return 0;
	}
	// Control test that the program is not in the current directory beforehand.
	if ( !access(program, F_OK) )
		errx(1, "test is being run in the wrong working directory");
	else if ( errno != ENOENT )
		errx(1, "control test: %s", program);
	posix_spawn_file_actions_t actions;
	if ( (errno = posix_spawn_file_actions_init(&actions)) )
		err(1, "posix_spawn_file_actions_init");
	int dirfd = open("spawn", O_RDONLY | O_DIRECTORY);
	if ( dirfd < 0 )
		err(1, "open: spawn");
	// Enter a subdirectory on spawn.
	if ( (errno = posix_spawn_file_actions_addfchdir(&actions, dirfd)) )
		err(1, "posix_spawn_file_actions_addfchdir");
	// Test that posix_spawnp searches PATH correctly with the new working
	// directory,
	if ( setenv("PATH", ".", 1) < 0 )
		err(1, "setenv");
	char* new_argv[] =
	{
		program,
		"success",
		NULL,
	};
	pid_t pid;
	if ( (errno = posix_spawnp(&pid, program, &actions, NULL, new_argv,
	                           environ)) )
		err(1, "posix_spawnp: %s", program);
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
