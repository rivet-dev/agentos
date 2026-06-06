/* Tests the behavior of reading the pty after the controller was closed,
 * and there is no data in flight. This should return EOF, which is the reason
 * a `poll(3)` would return `POLLIN`. */

#include "suite.h"

int main(void)
{
	pid_t session = fork();
	if ( session < 0 )
		err(1, "fork");

	if ( session == 0 )
	{
		// Avoid dying on SIGHUP when the controller is closed below.
		if ( signal(SIGHUP, SIG_IGN) == SIG_ERR )
			err(1, "signal");

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

		// Close the controlling terminal
		close(controller);

		uint8_t buf;
		ssize_t ret = read(pty, &buf, sizeof(buf));

		if ( ret < 0 )
			warn("read");
		else
			warnx("read == %zd", ret);

		return 0;
	}

	int status;
	if ( waitpid(session, &status, 0) < 0 )
		err(1, "waitpid");
	if ( WIFEXITED(status) )
		return WEXITSTATUS(status);
	else if ( WIFSIGNALED(status) )
		errx(1, "%s", strsignal(WTERMSIG(status)));
	else
		errx(1, "unknown exit: %#x", status);
	return 0;
}
