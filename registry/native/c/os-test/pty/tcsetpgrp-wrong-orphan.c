/* Test tcsetpgrp from an orphaned process group. */

#include "suite.h"

void on_sigttou(int signo)
{
	printf("SIGTTOU\n");
	fflush(stdout);
	_exit(1);
}

int main(void)
{
	int controller = posix_openpt(O_RDWR | O_NOCTTY);
	if ( controller < 0 )
		err(1, "posix_openpt");
	if ( grantpt(controller) < 0 )
		err(1, "grantpt");
	if ( unlockpt(controller) < 0 )
		err(1, "unlockpt");
	char* name = ptsname(controller);
	if ( !name )
		err(1, "unlockpt");
	pid_t session = fork();
	if ( session < 0 )
		err(1, "fork");
	if ( !session )
	{
		close(controller);
		if ( setsid() < 0 )
			err(1, "setsid");
		session = getpid();
		int pty = open(name, O_RDWR);
		if ( pty < 0 )
			err(1, "%s", name);
#ifdef TIOCSCTTY
		if ( ioctl(pty, TIOCSCTTY, 0) < 0 && errno != ENOTTY )
			err(1, "ioctl: TIOCSCTTY");
#endif
		int pipes[2];
		if ( pipe(pipes) < 0 )
			err(1, "pipe");
		pid_t member = fork();
		if ( member < 0 )
			err(1, "fork");
		if ( !member )
		{
			member = getpid();
			pid_t orphan = fork();
			if ( orphan < 0 )
				err(1, "fork");
			if ( orphan )
				exit(0);
			if ( setpgid(0, 0) < 0 )
				err(1, "setpgid");
			while ( getppid() == member )
				usleep(1000);
			int result = 0;
			// SIGTTOU is not supposed to happen because the EIO orphaned
			// process group case is supposed to happen, and SIGTTOU is neither
			// ignored nor blocked here.
			signal(SIGTTOU, on_sigttou);
			if ( tcsetpgrp(pty, getpgid(0)) < 0 )
				result = errno;
			orphan = getpid();
			if ( write(pipes[1], &result, sizeof(int)) <
			           (ssize_t) sizeof(int) )
				err(1, "write");
			exit(0);
		}
		close(pipes[1]);
		int result;
		if ( read(pipes[0], &result, sizeof(int)) < (ssize_t) sizeof(int) )
			err(1, "read");
		if ( 0 < result )
		{
			errno = result;
			err(1, "tcsetpgrp");
		}
		exit(0);
	}
	int status;
	if ( waitpid(session, &status, 0) < 0 )
		err(1, "waitpid");
	close(controller);
	if ( WIFEXITED(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else
		errx(1, "unknown exit: %#x", status);
	return 0;
}
