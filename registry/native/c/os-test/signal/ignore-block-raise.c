/* Test ignoring, blocking, and raising SIGUSR1. */

#include "signal.h"

int main(void)
{
	sigset_t sigusr1;
	sigemptyset(&sigusr1);
	sigaddset(&sigusr1, SIGUSR1);
	sigprocmask(SIG_BLOCK, &sigusr1, NULL);
	signal(SIGUSR1, SIG_IGN);
	raise(SIGUSR1);
	return 0;
}
