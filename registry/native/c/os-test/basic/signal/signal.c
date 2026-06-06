/* Test whether a basic signal invocation works. */

#include <signal.h>

#include "../basic.h"

void on_signal(int signo)
{
	(void) signo;
}

int main(void)
{
	if ( signal(SIGUSR1, on_signal) == SIG_ERR )
		err(1, "signal");
	return 0;
}
