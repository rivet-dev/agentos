/*[SPN]*/
/* Test whether a basic posix_spawn_file_actions_addopen invocation works. */

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
	// fd0 tests that existing fds are overwritten.
	// fd3 tests that a fd can be opened on spawn.
	// fd4 tests that a fd opened with CLOEXEC on spawn is closed afterwards.
	int fd0 = 0;
	int fd3 = 3;
	int fd4 = 4;
	if ( argc == 2 )
	{
		struct stat st0, st3, st4;
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "child invoked incorrectly");
		if ( fstat(fd0, &st0) < 0 )
			errx(1, "fd3 was not open in child");
		if ( fstat(fd3, &st3) < 0 )
			errx(1, "fd3 was not open in child");
		if ( !fstat(fd4, &st4) )
			errx(1, "fd4 was not closed in child");
		else if ( errno != EBADF )
			err(1, "child fstat");
		if ( st0.st_dev != st3.st_dev || st0.st_ino != st3.st_ino )
			errx(1, "fd0 and fd3 are not the sa<me file");
		return 0;
	}
	// Ensure fd 3 is available for our test.
	close(3);
	posix_spawn_file_actions_t actions;
	if ( (errno = posix_spawn_file_actions_init(&actions)) )
		err(1, "posix_spawn_file_actions_init");
	// Test that a file can be opened on spawn onto an existent fd.
	if ( (errno = posix_spawn_file_actions_addopen(&actions, fd0, argv[0],
	                                               O_RDONLY, 0)) )
		err(1, "posix_spawn_file_actions_addopen");
	// Test that a file can be opened on spawn on a non-existent fd.
	if ( (errno = posix_spawn_file_actions_addopen(&actions, fd3, argv[0],
	                                               O_RDONLY, 0)) )
		err(1, "posix_spawn_file_actions_addopen");
	// Test that a file can be opened on spawn on a non-existent fd but then
	// closed immediately if CLOEXEC is set.
	// "This transformation shall be as if the specified sequence of actions was
	//  performed exactly once, in the context of the spawned process (prior to
	//  execution of the new process image), in the order in which the actions
	//  were added to the object; additionally, when the new process image is
	//  executed, any file descriptor (from this new set) which has its
	//  FD_CLOEXEC flag set shall be closed (see posix_spawn())."
	if ( (errno = posix_spawn_file_actions_addopen(&actions, fd4, argv[0],
	                                               O_RDONLY | O_CLOEXEC, 0)) )
		err(1, "posix_spawn_file_actions_addopen");
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
