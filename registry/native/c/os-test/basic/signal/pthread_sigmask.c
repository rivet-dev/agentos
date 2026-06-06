/* Test whether a basic pthread_sigmask invocation works. */

#include <signal.h>

#include "../basic.h"

static volatile sig_atomic_t received;

void on_signal(int signo)
{
	received = signo;
}

int main(void)
{
	sigset_t set, oldset;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	if ( (errno = pthread_sigmask(SIG_BLOCK, &set, &oldset)) )
		err(1, "first pthread_sigmask");
	if ( signal(SIGUSR1, on_signal) == SIG_ERR )
		err(1, "signal");
	if ( raise(SIGUSR1) )
		err(1, "raise");
	if ( received )
		errx(1, "SIGUSR1 received while blocked");
	if ( (errno = pthread_sigmask(SIG_SETMASK, &oldset, NULL)) )
		err(1, "second pthread_sigmask");
	if ( received != SIGUSR1 )
		errx(1, "SIGUSR1 not received while unblocked");
	return 0;
}
