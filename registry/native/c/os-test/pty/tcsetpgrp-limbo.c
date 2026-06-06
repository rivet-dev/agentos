/* Test tcsetpgrp to a process in limbo (process group leader that has been
   awaited but still has a member). */

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
		// Clean up children once pipe is closed.
		int pipes[2];
		if ( pipe(pipes) < 0 )
			err(1, "pipe");
		pid_t zombie = fork();
		if ( zombie < 0 )
			err(1, "fork");
		char c;
		if ( !zombie )
		{
			close(pipes[1]);
			read(pipes[0], &c, 1);
			exit(0);
		}
		if ( setpgid(zombie, zombie) < 0 )
			err(1, "setpgid on zombie");
		pid_t member = fork();
		if ( member < 0 )
			err(1, "fork");
		if ( !member )
		{
			close(pipes[1]);
			read(pipes[0], &c, 1);
			exit(0);
		}
		if ( setpgid(member, zombie) < 0 )
			err(1, "setpgid on member");
		if ( kill(zombie, SIGKILL) < 0 )
			err(1, "kill zombie");
		int status;
		waitpid(zombie, &status, 0);
		if ( tcsetpgrp(pty, zombie) < 0 )
			err(1, "tcsetpgrp");
		pid_t tty_pgrp = tcgetpgrp(pty);
		if ( tty_pgrp < 0 )
			err(1, "tcgetpgrp");
		if ( tty_pgrp != zombie )
			errx(1, "tcgetpgrp() != getpgid(0)");
		if ( kill(member, SIGKILL) < 0 )
			err(1, "kill member");
		waitpid(member, &status, 0);
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
