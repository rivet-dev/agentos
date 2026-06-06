/* Test whether a basic pause invocation works. */

#include <sched.h>
#include <signal.h>
#include <unistd.h>

#include "../basic.h"

void on_alarm(int signum)
{
	(void) signum;
}

int main(void)
{
	struct sigaction sa = { .sa_handler = on_alarm };
	if ( sigaction(SIGUSR1, &sa, NULL) < 0 )
		err(1, "sigaction");
	pid_t parent = getpid();
	pid_t child = fork();
	if ( child < 0 )
		err(1, "fork");
	if ( !child )
	{
#ifndef __minix__
		sched_yield();
#endif
		while ( 1 )
		{
			if ( kill(parent, SIGUSR1) < 0 )
				exit(0);
			sleep(1);
		}
	}
	int ret = pause();
	int errnum = errno;
	kill(child, SIGKILL);
	errno = errnum;
	if ( ret < 0 && errno == EINTR )
		;
	else if ( ret < 0 )
		err(1, "pause");
	else if ( ret == 0 )
		errx(1, "pause() did not fail");
	return 0;
}
