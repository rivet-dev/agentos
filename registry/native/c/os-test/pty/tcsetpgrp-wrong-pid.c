/* Test tcsetpgrp with a process group that doesn't exist. */

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
		pid_t bad_pid = 2000000000;
		while ( !kill(bad_pid, 0) )
			bad_pid--;
		if ( tcsetpgrp(pty, bad_pid) < 0 )
			err(1, "tcsetpgrp");
		pid_t tty_pgrp = tcgetpgrp(pty);
		if ( tty_pgrp < 0 )
			err(1, "tcgetpgrp");
		if ( tty_pgrp != bad_pid )
			errx(1, "tcgetpgrp() != bad_pid");
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
