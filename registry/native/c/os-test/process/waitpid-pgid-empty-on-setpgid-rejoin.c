/* Test if waitpid wakes if there suddenly are no more children left in the
   requested process group because the child rejoined the old process group. */

#include "process.h"

void on_signal(int signo)
{
	(void) signo;
	fprintf(stderr, "SIGALRM\n");
	_exit(1);
}

int main(void)
{
	int alive_fd[2], pgid_fd[2];
	if ( pipe(alive_fd) < 0 || pipe(pgid_fd) < 0 )
		err(1, "pipe");
#ifdef __APPLE__
	pid_t original_pid = getpid();
#endif
	// Make a process group that is not a direct child so waitpid can't see it.
	pid_t pgid = fork();
	if ( pgid < 0 )
		err(1, "fork");
	if ( !pgid )
	{
		close(alive_fd[1]);
		close(pgid_fd[0]);
		// Double fork to not be a direct child.
		pgid = fork();
		if ( pgid < 0 )
			err(1, "fork");
		if ( pgid )
			_exit(0);
		// And become a process group leader.
		if ( setpgid(0, 0) < 0 )
			err(1, "setpgid");
		// Tell the original process what our process group id is.
		pgid = getpgid(0);
		if ( write(pgid_fd[1], &pgid, sizeof(pgid)) < (ssize_t) sizeof(pgid) )
			err(1, "write");
		close(pgid_fd[1]);
#ifdef __APPLE__
		// macOS seems to hang in the test, despite SIGALRM, so give it an extra
		// fallback timeout kick.
		alarm(4);
		sleep(3);
		kill(original_pid, SIGKILL);
		_exit(1);
#endif
		// Stay alive as long as the alive pipe is connected to the original
		// process, so we exit when it does. It will never write here.
		char c;
		read(alive_fd[0], &c, 1);
		_exit(0);
	}
	// Wait for the process group to exist and receive it's id.
	if ( read(pgid_fd[0], &pgid, sizeof(pgid)) < (ssize_t) sizeof(pgid) )
		err(1, "read");
	// Make a child that will leave it's process group after a moment.
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
#ifdef __APPLE__
		alarm(4);
#endif
		// While we sleep, the parent will place us in the process group and go
		// to sleep inside waitpid.
		sleep(1);
		// Leave the process group by rejoining the parent's process group.
		if ( setpgid(getpid(), getpgid(getppid())) < 0 )
			err(1, "setpgid rejoin");
		// Stay alive as long as the alive pipe is connected to the original
		// process, so we exit when it does. It will never write here.
		char c;
		read(alive_fd[0], &c, 1);
		_exit(0);
	}
	// Place the child inside the indirect process group.
	if ( setpgid(child, pgid) < 0 )
		err(1, "setpgid of child into pgid");
	// Time out if the process hangs inside waitpid.
	signal(SIGALRM, on_signal);
	alarm(2);
	int status;
	// waitpid is supposed to ECHLD here when the child changes its pgid.
	pid_t result = waitpid(-pgid, &status, 0);
	if ( result < 0 )
		err(1, "waitpid");
	else if ( result == 0 )
		errx(1, "waitpid() == 0");
	else if ( result == child )
		errx(1, "waitpid returned child");
	else
		errx(1, "waitpid returned strange child");
	return 0;
}
