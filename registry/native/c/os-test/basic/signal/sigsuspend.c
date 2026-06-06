/* Test whether a basic sigsuspend invocation works. */

#include <signal.h>

#include "../basic.h"

volatile sig_atomic_t signaled = 0;

/* Install a handler, as sigsuspend only gets interrupted by delivery of a
 * signal that either terminates or executes a signal handler. */
static void handler(int x)
{
	(void) x;
	signaled = 1;
}

int main(void)
{
	struct sigaction sa = { 0 };
	sa.sa_handler = handler;
	sa.sa_flags = 0;
	sigemptyset(&sa.sa_mask);
	sigaction(SIGUSR1, &sa, NULL);

	sigset_t oldmask;
	sigset_t sigmask;
	sigemptyset(&sigmask);
	sigaddset(&sigmask, SIGUSR1);

	// Block SIGUSR1 and obtain the current signal mask with SIGUSR1 unblocked
	sigprocmask(SIG_BLOCK, &sigmask, &oldmask);
	sigdelset(&oldmask, SIGUSR1);

	// Queue a SIGUSR1 signal which will not be delivered.
	raise(SIGUSR1);

	// Wait for a signal to arrive that is not masked by oldmask, which
	// should be SIGUSR1
	if ( !sigsuspend(&oldmask) )
		errx(1, "sigsuspend succeeded");
	if ( errno != EINTR )
		err(1, "sigsuspend");

	if ( signaled == 0 )
		errx(1, "signal handler was not executed");

	return 0;
}
