/* Test tcgetpgrp without controlling the terminal. */

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
	// Clean up children when cleanup_pipe closes.
	int notify_pipe[2];
	int cleanup_pipe[2];
	if ( pipe(notify_pipe) < 0 || pipe(cleanup_pipe) < 0 )
		err(1, "pipe");
	pid_t session = fork();
	if ( session < 0 )
		err(1, "fork");
	if ( !session )
	{
		close(cleanup_pipe[1]);
		close(notify_pipe[0]);
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
		char c = 'x';
		if ( write(notify_pipe[1], &c, 1) < 0 )
			err(1, "write");
		if ( read(cleanup_pipe[0], &c, 1) < 0 )
			err(1, "read");
		exit(0);
	}
	close(notify_pipe[1]);
	char c;
	if ( read(notify_pipe[0], &c, 1) == 1 )
	{
		int pty = open(name, O_RDWR | O_NOCTTY);
		if ( pty < 0 )
			err(1, "%s", name);
		// ENOTTY is supposed to happen here due to the wrong controlling tty.
		pid_t tty_pgrp = tcgetpgrp(pty);
		if ( tty_pgrp < 0 )
			err(1, "tcgetpgrp");
		if ( tty_pgrp != session )
			errx(1, "tcgetpgrp() != session");
		return 0;
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
