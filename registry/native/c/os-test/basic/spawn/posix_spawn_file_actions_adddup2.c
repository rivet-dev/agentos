/*[SPN]*/
/* Test whether a basic posix_spawn_file_actions_adddup2 invocation works. */

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
	// fd3 tests that a fd with O_CLOEXEC is closed.
	// fd4 tests that a fd with O_CLOEXEC is not closed, if self-duplicated
	// with posix_spawn_file_actions_adddup2.
	// fd5 tests that posix_spawn_file_actions_adddup2 duplicates a fd, even
	// if the source is O_CLOEXEC.
	int fd3 = 3;
	int fd4 = 4;
	int fd5 = 5;
	if ( argc == 2 )
	{
		if ( strcmp(argv[1], "success") != 0 )
			err(1, "child invoked incorrectly");
		struct stat st;
		if ( !fstat(fd3, &st) )
			errx(1, "fd3 was not closed in child");
		else if ( errno != EBADF )
			err(1, "child fstat");
		if ( fstat(fd4, &st) < 0 )
			errx(1, "fd4 was not open in child");
		if ( fstat(fd5, &st) < 0 )
			errx(1, "fd5 was not open in child");
		return 0;
	}
	// Ensure fd 3, 4, and 5 are available for our test.
	close(fd3);
	close(fd4);
	close(fd5);
	// Open file descriptor 3 with O_CLOEXEC to be closed on spawn.
	fd3 = open("spawn", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
	if ( fd3 < 0 )
		err(1, "open: spawn");
	if ( fd3 != 3 )
		errx(1, "open did not return 3");
	struct stat st;
	if ( fstat(fd3, &st) < 0 )
		errx(1, "control fstat failed");
	// Open file descriptor 5 as a dup of fd 4 and ensure CLOEXEC is set, but
	// this fd will be self-duplicated with posix_spawn_file_actions_adddup2 and
	// should not close. dup3() can't be used here yet, because it's not
	// portable enough to older systems.
	fd4 = dup(fd3);
	if ( fd4 < 0 )
		err(1, "dup");
	if ( fd4 != 4 )
		errx(1, "dup did not return 4");
	if ( fcntl(fd4, F_SETFD, FD_CLOEXEC) < 0 )
		err(1, "fcntl: F_SETFD");
	posix_spawn_file_actions_t actions;
	if ( (errno = posix_spawn_file_actions_init(&actions)) )
		err(1, "posix_spawn_file_actions_init");
	// Test that a self-dup will unset the CLOEXEC flag even though that dup2
	// does not unset that flag.
	// "If fildes and newfildes are equal, then the action shall ensure that
	//  newfildes is inherited by the new process with FD_CLOEXEC clear, even if
	//  the FD_CLOEXEC flag of fildes is set at the time the new process is
	//  spawned, and even though dup2() would not make such a change."
	if ( (errno = posix_spawn_file_actions_adddup2(&actions, fd4, fd4)) )
		err(1, "posix_spawn_file_actions_adddup2");
	// Test that a file descriptor can be duplicated on spawn.
	if ( (errno = posix_spawn_file_actions_adddup2(&actions, fd4, fd5)) )
		err(1, "posix_spawn_file_actions_adddup2");
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
