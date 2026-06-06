/* Test tcgetpgrp after the process group has exited. */

#include "suite.h"

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
		pid_t tty_pgrp = tcgetpgrp(pty);
		if ( tty_pgrp < 0 )
			err(1, "tcgetpgrp");
		if ( tty_pgrp != session )
			errx(1, "tcgetpgrp() != getpgid(0)");
		// Clean up children once pipe is closed.
		int pipes[2];
		if ( pipe(pipes) < 0 )
			err(1, "pipe");
		pid_t pgid = fork();
		if ( pgid < 0 )
			err(1, "fork");
		char c;
		if ( !pgid )
		{
			close(pipes[1]);
			read(pipes[0], &c, 1);
			exit(0);
		}
		if ( setpgid(pgid, pgid) < 0 )
			err(1, "setpgid on child");
		if ( tcsetpgrp(pty, pgid) < 0 )
			err(1, "tcsetpgrp");
		close(pipes[1]);
		int status;
		waitpid(pgid, &status, 0);
		tty_pgrp = tcgetpgrp(pty);
		if ( tty_pgrp < 0 )
			err(1, "tcgetpgrp");
		if ( tty_pgrp == pgid )
			printf("unchanged\n");
		else if ( tty_pgrp == getpgid(0) )
			printf("parent pgid\n");
		else if ( tty_pgrp == session )
			printf("session\n");
		else if ( 100000 <= tty_pgrp && kill(tty_pgrp, 0) < 0 )
			printf("reserved\n");
		else
			printf("%li\n", (long) tty_pgrp);
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
