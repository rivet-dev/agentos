/* Test whether a basic raise invocation works. */

#include <signal.h>

#include "../basic.h"

static volatile sig_atomic_t received;

void on_signal(int signo)
{
	received = signo;
}

int main(void)
{
	if ( signal(SIGUSR1, on_signal) == SIG_ERR )
		err(1, "signal");
	if ( raise(SIGUSR1) )
		err(1, "raise");
	if ( received != SIGUSR1 )
		errx(1, "SIGUSR1 not received while unblocked");
	return 0;
}
