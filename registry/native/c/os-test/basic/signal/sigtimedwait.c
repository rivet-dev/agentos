/* Test whether a basic sigtimedwait invocation works. */

#include <signal.h>
#include <stdint.h>
#include <unistd.h>

#include "../basic.h"

static const uint64_t magic = 0x012345678ABCDEF;

int main(void)
{
	struct timespec delay = { .tv_sec = 0, .tv_nsec = 1 };

	siginfo_t info;
	sigset_t set;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	if ( sigprocmask(SIG_BLOCK, &set, NULL) < 0 )
		err(1, "sigprocmask");

	if ( sigtimedwait(&set, &info, &delay) < 0 )
	{
		if ( errno != EAGAIN )
			err(1, "sigtimedwait");
	}
	else
		errx(1, "sigtimedwait unexpectedly got signal");

	union sigval sv;
	sv.sival_ptr = (void*) (uintptr_t) magic;
	if ( sigqueue(getpid(), SIGUSR1, sv) < 0 )
		err(1, "sigqueue");
	if ( sigtimedwait(&set, &info, &delay) < 0 )
		err(1, "sigtimedwait");
	if ( info.si_signo != SIGUSR1 )
		errx(1, "sigqueue si_signo != SIGUSR1");
	if ( info.si_code != SI_QUEUE )
		errx(1, "sigqueue si_code != SI_QUEUE");
	if ( info.si_pid != getpid() )
		errx(1, "sigqueue si_pid != getpid()");
	if ( info.si_uid != getuid() )
		errx(1, "sigqueue si_uid != getuid()");
	if ( info.si_value.sival_ptr != (void*) (uintptr_t) magic )
		errx(1, "sigqueue si_value != magic");

	if ( kill(getpid(), SIGUSR1) )
		err(1, "kill");
	if ( sigtimedwait(&set, &info, &delay) < 0 )
		err(1, "sigtimedwait");
	if ( info.si_signo != SIGUSR1 )
		errx(1, "kill si_signo != SIGUSR1");
	if ( info.si_code != SI_USER )
		errx(1, "kill si_code != SI_USER)");
	if ( info.si_pid != getpid() )
		errx(1, "kill si_pid != getpid()");
	if ( info.si_uid != getuid() )
		errx(1, "kill si_uid != getuid()");

	return 0;
}
