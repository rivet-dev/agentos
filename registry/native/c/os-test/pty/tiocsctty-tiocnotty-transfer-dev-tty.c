/* See if a pty can be transferred to another session if relinquished with
   TIOCNOTTY on /dev/tty. */

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
		close(controller);
		close(cleanup_pipe[1]);
		close(notify_pipe[0]);
		if ( setsid() < 0 )
			err(1, "setsid");
		session = getpid();
		int pty = open(name, O_RDWR | O_NOCTTY);
		if ( pty < 0 )
			err(1, "%s", name);
#ifdef TIOCSCTTY
		if ( ioctl(pty, TIOCSCTTY, 0) < 0 )
			err(1, "ioctl: TIOCSCTTY");
#else
		errx(1, "no TIOCSCTTY");
#endif
		signal(SIGHUP, SIG_IGN);
#ifdef TIOCNOTTY
		// Get rid of the controlling terminal immediately.
		int tty = open("/dev/tty", O_RDWR | O_NOCTTY);
		if ( tty < 0 )
			err(1, "/dev/tty");
		// This may fail EINVAL because we're the session leader.
		if ( ioctl(tty, TIOCNOTTY, 0) < 0 )
			err(1, "ioctl: TIOCNOTTY");
#else
		errx(1, "no TIOCNOTTY");
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
	if ( read(notify_pipe[0], &c, 1) < 1 )
	{
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
	pid_t other = fork();
	if ( other < 0 )
		err(1, "fork");
	if ( !other )
	{
		close(cleanup_pipe[1]);
		close(controller);
		if ( setsid() < 0 )
			err(1, "setsid");
		other = getpid();
		int pty = open(name, O_RDWR | O_NOCTTY);
		if ( pty < 0 )
			err(1, "%s", name);
#ifdef TIOCSCTTY
		// It should be possible to control the terminal now.
		if ( ioctl(pty, TIOCSCTTY, 0) < 0 )
			err(1, "transfer ioctl: TIOCSCTTY");
#else
		errx(1, "no TIOCSCTTY");
#endif
#ifdef __minix__
		if ( open("/dev/tty", O_RDWR) < 0 )
			err(1, "/dev/tty");
#else
		errno = 0;
		pid_t tty_sid = tcgetsid(pty);
		if ( tty_sid < 0 )
			err(1, "tcgetsid");
		else if ( tty_sid == session )
			errx(1, "tcgetsid() == old session");
		else if ( tty_sid != other )
			errx(1, "tcgetsid() == %li?", tty_sid);
#endif
		return 0;
	}
	int status;
	if ( waitpid(other, &status, 0) < 0 )
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
