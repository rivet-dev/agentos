/* Tests the behavior of poll(3) on a pty before its controller
 * is closed. Here, a non-blocking poll should not return anything - there
 * is no data to be read, no hangup occured, and no `read(3)` or `write(3)`
 * should return an error. We also include `POLLOUT` because it is mutually
 * exclusive with `POLLHUP`. */

#include "suite.h"

#include <stdint.h>
#include <poll.h>
#include <unistd.h>

int main(void)
{
	pid_t session = fork();
	if ( session < 0 )
		err(1, "fork");

	if ( session == 0 )
	{
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

		struct pollfd pfd;
		pfd.fd = pty;
		pfd.events = POLLIN; // POLLERR and POLLHUP are always reported.
		pfd.revents = 0;

		int ret = poll(&pfd, 1, 0);
		if ( ret < 0 )
			err(1, "poll");

		fprintf(stderr, "0");
		if ( pfd.revents & POLLIN )
			fprintf(stderr, " | POLLIN");
		if ( pfd.revents & POLLOUT )
			fprintf(stderr, " | POLLOUT");
		if ( pfd.revents & POLLERR )
			fprintf(stderr, " | POLLERR");
		if ( pfd.revents & POLLHUP )
			fprintf(stderr, " | POLLHUP");
		if ( pfd.revents & POLLNVAL )
			fprintf(stderr, " | POLLNVAL");
		fprintf(stderr, "\n");

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
