/* Test whether a basic sigismember invocation works. */

#include <signal.h>

#include "../basic.h"

int main(void)
{
	sigset_t set;
	sigemptyset(&set);
	if ( sigismember(&set, SIGUSR1) != 0 ||
	     sigismember(&set, SIGUSR2) != 0 )
		errx(1, "control test failed");
	if ( sigaddset(&set, SIGUSR1) < 0 )
		err(1, "sigaddset");
	if ( sigismember(&set, SIGUSR1) != 1 )
		errx(1, "SIGUSR1 was not set");
	if ( sigismember(&set, SIGUSR2) != 0 )
		errx(1, "SIGUSR2 was not unset");
	if ( 0 <= sigismember(&set, -1) )
		errx(1, "sigismember did not fail on a negative signal");
	if ( 0 <= sigismember(&set, 1024 * 1024) )
		errx(1, "sigismember did not fail on a too large signal");
	return 0;
}
