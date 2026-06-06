/* Test ignoring and raising SIGUSR1. */

#include "signal.h"

int main(void)
{
	signal(SIGUSR1, SIG_IGN);
	raise(SIGUSR1);
	return 0;
}
