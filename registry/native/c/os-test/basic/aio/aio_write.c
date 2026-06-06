/* Test whether a basic aio_write invocation works. */

#include <aio.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cond = PTHREAD_COND_INITIALIZER;

static void on_io(union sigval sigval)
{
	pthread_mutex_lock(&mutex);
	pthread_cond_signal((pthread_cond_t*) sigval.sival_ptr);
	pthread_mutex_unlock(&mutex);
}

int main(void)
{
	FILE* fp = tmpfile();
	if ( !fp )
		err(1, "tmpfile");
	if ( fputs("foobar", fp) == EOF || ferror(fp) || fflush(fp) == EOF )
		err(1, "fputs");
	int fd = fileno(fp);
	char buffer[6] = {'F', 'O', 'O', 'B', 'A', 'R'};
	struct aiocb aio =
	{
		.aio_fildes = fd,
		.aio_offset = 1,
		.aio_buf = buffer,
		.aio_nbytes = sizeof(buffer),
		.aio_sigevent =
		{
			.sigev_notify = SIGEV_THREAD,
			.sigev_signo = SIGUSR1,
			.sigev_notify_function = on_io,
			.sigev_value = { .sival_ptr = &cond },
		},
	};
	pthread_mutex_lock(&mutex);
	if ( aio_write(&aio) < 0 )
		err(1, "aio_write");
	if ( (errno = aio_error(&aio)) && errno != EINPROGRESS )
		err(1, "aio_error");
	pthread_cond_wait(&cond, &mutex);
	pthread_mutex_unlock(&mutex);
	if ( (errno = aio_error(&aio)) )
		err(1, "aio_error");
	ssize_t ret = aio_return(&aio);
	if ( ret < 0 )
		errx(1, "aio_return() < 0");
	if ( ret != sizeof(buffer) )
		errx(1, "aio_return() != sizeof(buffer)");
	char check[16];
	if ( pread(fd, check, sizeof(check), 0) != 7 )
		errx(1, "pread() != 7");
	if ( memcmp(check, "fFOOBAR", 7) != 0 )
		errx(1, "aio_read did not read \"fFOOBAR\"");
	return 0;
}
