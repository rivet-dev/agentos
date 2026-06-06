/* We fork off a child before putting it in its own session, and then testing
 * that closing the controller fd from the parent sends a `SIGHUP` to
 * the child, which then terminates (the default action for that signal).
 * The parent uses `waitpid(3)` to check the termination reason of the child. */

#include "suite.h"

int main(void)
{
	pid_t session = fork();
	if ( session < 0 )
		err(1, "fork");

	if ( session == 0 )
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
			err(1, "ptsname");

		if ( setsid() == (pid_t) -1 )
			err(1, "setsid");

		int pty = open(name, O_RDWR);
		if ( pty == -1 )
			err(1, "open(pty)");

#ifdef TIOCSCTTY
		if ( ioctl(pty, TIOCSCTTY, 0) < 0 && errno != ENOTTY )
			err(1, "ioctl: TIOCSCTTY");
#endif
		if ( tcsetpgrp(pty, getpid()) < 0 )
			err(1, "tcsetpgrp");

		// SIGHUP is sent to the foreground process group here, which contains
		// this process, which should now be dead with SIGHUP instead of
		// executing exit(0).
		close(controller);

		return 0;
	}

	int status;
	if ( waitpid(session, &status, 0) < 0 )
		err(1, "waitpid");

	if ( WIFEXITED(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
	{
		if ( WTERMSIG(status) == SIGHUP )
			warnx("SIGHUP");
		else
			errx(1, "%s", strsignal(WTERMSIG(status)));
	}
	else
		errx(1, "unknown exit: %#x", status);
	return 0;
}
