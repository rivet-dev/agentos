/* Test whether a basic sigaddset invocation works. */

#include <signal.h>

#include "../basic.h"

int main(void)
{
	sigset_t set;
	sigemptyset(&set);
	if ( sigismember(&set, SIGUSR1) != 0 )
		errx(1, "control test failed");
	if ( sigaddset(&set, SIGUSR1) < 0 )
		err(1, "sigaddset");
	if ( sigismember(&set, SIGUSR1) != 1 )
		errx(1, "signal was not set");
	if ( !sigaddset(&set, -1) )
		errx(1, "sigaddset did not fail on a negative signal");
	if ( !sigaddset(&set, 1024 * 1024) )
		errx(1, "sigaddset did not fail on a too large signal");
	return 0;
}
