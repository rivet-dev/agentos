/* Test whether a basic timer_settime invocation works. */

#include <signal.h>
#include <time.h>

#include "../basic.h"

static volatile sig_atomic_t received;

void on_signal(int signo)
{
	received = signo;
}

int main(void)
{
	struct sigaction sa = { .sa_handler = on_signal };
	if ( sigaction(SIGUSR1, &sa, NULL) < 0 )
		err(1, "sigaction");
	sigset_t set, oldset;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	sigprocmask(SIG_BLOCK, &set, &oldset);
	struct sigevent event =
	{
		.sigev_notify = SIGEV_SIGNAL,
		.sigev_signo = SIGUSR1,
		.sigev_value = { .sival_int = 42 },
	};
	timer_t timer;
	if ( timer_create(CLOCK_MONOTONIC, &event, &timer) < 0 )
		err(1, "timer_create");
	struct itimerspec its = { .it_value = { .tv_sec = 0, .tv_nsec = 1 } };
	if ( timer_settime(timer, 0, &its, NULL) < 0 )
		err(1, "timer_settime");
	sigsuspend(&oldset);
	if ( received != SIGUSR1 )
		err(1, "timer did not send signal");
	return 0;
}
