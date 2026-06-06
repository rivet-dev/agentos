/* Test whether a basic sigqueue invocation works. */

#include <signal.h>
#include <stdint.h>
#include <unistd.h>

#include "../basic.h"

static const uint64_t magic = 0x012345678ABCDEF;
static volatile sig_atomic_t received = 0;

static void on_signal(int signo, siginfo_t* info, void* uctx)
{
	(void) uctx;
	received = signo;
	if ( !info )
		err(1, "siginfo is NULL");
	if ( info->si_signo != SIGUSR1 )
		errx(1, "si_signo != SIGUSR1");
	if ( info->si_code != SI_QUEUE )
		errx(1, "si_signo != SI_QUEUE");
	if ( info->si_pid != getpid() )
		errx(1, "si_pid != getpid()");
	if ( info->si_uid != getuid() )
		errx(1, "si_uid != getuid()");
	if ( info->si_value.sival_ptr != (void*) (uintptr_t) magic )
		errx(1, "si_value != magic");
}

int main(void)
{
	struct sigaction sa = { .sa_sigaction = on_signal, .sa_flags = SA_SIGINFO };
	if ( sigaction(SIGUSR1, &sa, NULL) < 0 )
		err(1, "sigaction");
	union sigval sv;
	sv.sival_ptr = (void*) (uintptr_t) magic;
	if ( sigqueue(getpid(), SIGUSR1, sv) < 0 )
		err(1, "sigqueue");
	if ( received != SIGUSR1 )
		errx(1, "signal was not received");
	return 0;
}
