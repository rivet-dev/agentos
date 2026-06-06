/* Test whether a basic sigwaitinfo invocation works. */

#include <signal.h>
#include <stdint.h>
#include <unistd.h>

#include "../basic.h"

static const uint64_t magic = 0x012345678ABCDEF;

int main(void)
{
	siginfo_t info;
	sigset_t set;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	if ( sigprocmask(SIG_BLOCK, &set, NULL) < 0 )
		err(1, "sigprocmask");

	union sigval sv;
	sv.sival_ptr = (void*) (uintptr_t) magic;
	if ( sigqueue(getpid(), SIGUSR1, sv) < 0 )
		err(1, "sigqueue");
	if ( sigwaitinfo(&set, &info) < 0 )
		err(1, "sigwaitinfo");
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
	if ( sigwaitinfo(&set, &info) < 0 )
		err(1, "sigwaitinfo");
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
