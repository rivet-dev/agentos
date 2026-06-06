/* Test whether a basic sigaction invocation works. */

#include <signal.h>

#include "../basic.h"

static volatile sig_atomic_t received1;
static volatile sig_atomic_t received2;

void on_signal(int signo)
{
	if ( signo == SIGUSR1 )
		received1 = 1;
	if ( signo == SIGUSR2 )
		received2 = 1;
	if ( signo == SIGUSR1 )
	{
		raise(SIGUSR2);
		if ( received2 )
			errx(1, "SIGUSR2 delivered while blocked inside SIGUSR1");
	}
}

int main(void)
{
	struct sigaction sa = { .sa_handler = on_signal };
	sigaddset(&sa.sa_mask, SIGUSR1);
	sigaddset(&sa.sa_mask, SIGUSR2);
	if ( sigaction(SIGUSR1, &sa, NULL) < 0 ||
	     sigaction(SIGUSR2, &sa, NULL) < 0 )
		err(1, "sigaction");
	if ( raise(SIGUSR1) )
		err(1, "raise");
	if ( !received1 )
		errx(1, "SIGUSR1 not received");
	if ( !received2 )
		errx(1, "SIGUSR2 not received");
	return 0;
}
