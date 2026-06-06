/*[SPN PS]*/
/* Test whether a basic posix_spawnattr_setschedpolicy invocation works. */

#include <sys/wait.h>

#include <pthread.h>
#include <sched.h>
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
	// Test setting the scheduler policy. Obtaining the current settings may
	// fail with EPERM, which is ignored.
	int policy;
	struct sched_param param = {0};
	if ( (policy = sched_getscheduler(0)) < 0 &&
		 (errno = pthread_getschedparam(pthread_self(), &policy, &param)) )
		policy = SCHED_OTHER;
	posix_spawnattr_t attr;
	if ( (errno = posix_spawnattr_init(&attr)) )
		err(1, "posix_spawnattr_init");
	if ( (errno = posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSCHEDULER)) )
		err(1, "posix_spawnattr_setflags");
	if ( (errno = posix_spawnattr_setschedpolicy(&attr, policy)) )
		err(1, "posix_spawnattr_setschedpolicy");
	char* new_argv[] =
	{
		argv[0],
		"success",
		NULL,
	};
	pid_t pid;
	if ( (errno = posix_spawn(&pid, argv[0], NULL, &attr, new_argv, environ)) )
	{
		if ( errno == EPERM )
			return 0;
		err(1, "posix_spawn: %s", new_argv[0]);
	}
	int status;
	if ( waitpid(pid, &status, 0) < 0 )
		err(1, "waitpid");
	if ( WIFEXITED(status) )
	{
		// Exit 127 may mean EPERM inside the new process.
		if ( WEXITSTATUS(status) == 127 )
			return 0;
		return WEXITSTATUS(status);
	}
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else
		errx(1, "unknown exit: %#x", status);
	return 0;
}
