/*[FSC|SIO]*/
/* Test whether a basic aio_fsync invocation works. */

#include <aio.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

static volatile sig_atomic_t got_signal;

static void on_signal(int signo)
{
	got_signal = signo;
}

int main(void)
{
	alarm(1); // DragonFly, Hurd
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fputs("foobar", fp) == EOF || ferror(fp) || fflush(fp) == EOF )
		err(1, "fputs");
	sigset_t set, oldset;
	sigemptyset(&set);
	sigaddset(&set, SIGUSR1);
	pthread_sigmask(SIG_BLOCK, &set, &oldset);
	struct sigaction sa = { .sa_handler = on_signal };
	if ( sigaction(SIGUSR1, &sa, NULL) < 0 )
		err(1, "sigaction");
	int fd = fileno(fp);
	struct aiocb aio =
	{
		.aio_fildes = fd,
		.aio_sigevent =
		{
			.sigev_notify = SIGEV_SIGNAL,
			.sigev_signo = SIGUSR1,
		},
	};
	if ( aio_fsync(O_SYNC, &aio) < 0 )
		err(1, "aio_fsync");
	sigsuspend(&oldset);
	if ( !got_signal )
		errx(1, "did not get signal");
	if ( (errno = aio_error(&aio)) )
		err(1, "aio_error");
	ssize_t ret = aio_return(&aio);
	if ( ret < 0 )
		errx(1, "aio_return() < 0");
	if ( ret != 0 )
		errx(1, "aio_return() != 0");
	return 0;
}
