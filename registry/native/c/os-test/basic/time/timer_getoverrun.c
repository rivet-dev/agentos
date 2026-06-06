/* Test whether a basic timer_getoverrun invocation works. */

#include <signal.h>
#include <time.h>

#include "../basic.h"

static timer_t timer;
static volatile sig_atomic_t received;
static volatile sig_atomic_t overrun;

void on_signal(int signo)
{
	received = signo;
	if ( (overrun = timer_getoverrun(timer)) < 0 )
		err(1, "timer_getoverrun");
	int control = timer_getoverrun(timer);
	if ( control < overrun )
		errx(1, "timer_getoverrun reset unexpectedly");
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
	if ( timer_create(CLOCK_MONOTONIC, &event, &timer) < 0 )
		err(1, "timer_create");
	struct timespec now;
	if ( clock_gettime(CLOCK_MONOTONIC, &now) < 0 )
		err(1, "clock_gettime");
	struct timespec next = now;
	next.tv_nsec += 200000000L; // 200 ms
	if ( 1000000000L <= next.tv_nsec )
	{
		next.tv_nsec -= 1000000000L;
		next.tv_sec++;
	}
	struct itimerspec its =
	{
		.it_value = next,
		.it_interval =  { .tv_sec = 0, .tv_nsec = 100000000L }, // 100 ms
	};
	if ( timer_settime(timer, TIMER_ABSTIME, &its, NULL) < 0 )
		err(1, "timer_settime");
	struct timespec expiration = now;
	expiration.tv_nsec += 550000000L; // 550 ms
	if ( 1000000000L <= expiration.tv_nsec )
	{
		expiration.tv_nsec -= 1000000000L;
		expiration.tv_sec++;
	}
	if ( clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &expiration,
	                     NULL) < 0 )
		err(1, "clock_nanosleep");
	sigsuspend(&oldset);
	if ( received != SIGUSR1 )
		err(1, "timer did not send signal");
	if ( overrun < 3 )
		errx(1, "timer_getoverrun() was less than three (%d)", overrun);
	sigsuspend(&oldset);
	if ( 3 <= overrun  )
		errx(1, "timer_getoverrun() did not reset (%d)", overrun);
	return 0;
}
