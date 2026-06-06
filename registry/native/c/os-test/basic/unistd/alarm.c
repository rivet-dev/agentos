/* Test whether a basic alarm invocation works. */

#include <signal.h>
#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

static void handler(int signum)
{
	(void) signum;
	exit(0);
}

int main(void)
{
	signal(SIGALRM, handler);
	alarm(1);
	sleep(2);
	err(1, "SIGALARM did not occur");
}
