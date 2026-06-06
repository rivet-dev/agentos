/* Test if the controlling tty can be set with open or TIOCSCTTY (if needed). */

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
		if ( tcgetsid(pty) < 0 && errno == ENOTTY )
		{
			if ( ioctl(pty, TIOCSCTTY, 0) < 0 )
				err(1, "ioctl: TIOCSCTTY");
		}
#endif
#ifdef __minix__
		if ( open("/dev/tty", O_RDWR) < 0 )
			err(1, "/dev/tty");
#else
		errno = 0;
		pid_t tty_sid = tcgetsid(pty);
		if ( tty_sid < 0 )
			err(1, "tcgetsid");
		else if ( tty_sid != session )
			errx(1, "tcgetsid() == %li?", tty_sid);
#endif
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
