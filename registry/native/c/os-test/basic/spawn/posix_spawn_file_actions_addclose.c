/*[SPN]*/
/* Test whether a basic posix_spawn_file_actions_addclose invocation works. */

#include <sys/stat.h>
#include <sys/wait.h>

#include <fcntl.h>
#include <spawn.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include "../basic.h"

extern char** environ;

int main(int argc, char* argv[])
{
	if ( argc == 2 )
	{
		int fd = atoi(argv[1]);
		struct stat st;
		if ( !fstat(fd, &st) )
			errx(1, "fd was not closed in child");
		else if ( errno != EBADF )
			err(1, "child fstat");
		return 0;
	}
	// Open a file descriptor to be closed on spawn.
	int fd = open("spawn", O_RDONLY | O_DIRECTORY);
	if ( fd < 0 )
		err(1, "open: spawn");
	struct stat st;
	if ( fstat(fd, &st) < 0 )
		errx(1, "control fstat failed");
	posix_spawn_file_actions_t actions;
	if ( (errno = posix_spawn_file_actions_init(&actions)) )
		err(1, "posix_spawn_file_actions_init");
	// Close the file descriptor on spawn.
	if ( (errno = posix_spawn_file_actions_addclose(&actions, fd)) )
		err(1, "posix_spawn_file_actions_addclose");
	// Tell the child which fd is supposed to be closed.
	char fdstr[sizeof(fd) * 3];
	snprintf(fdstr, sizeof(fdstr), "%d", fd);
	char* new_argv[] =
	{
		argv[0],
		fdstr,
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
