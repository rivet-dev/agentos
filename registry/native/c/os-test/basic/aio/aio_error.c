/* Test whether a basic aio_error invocation works. */

#include <aio.h>
#include <signal.h>
#include <stdio.h>
#include <unistd.h>

#include "../basic.h"

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
		.aio_offset = -9000,
		.aio_buf = buffer,
		.aio_nbytes = sizeof(buffer),
		.aio_sigevent = { .sigev_notify = SIGEV_NONE },
	};
	if ( aio_write(&aio) < 0 )
	{
		if ( errno == EINVAL || errno == EFBIG )
			return 0;
		err(1, "aio_write");
	}
	if ( (errno = aio_error(&aio)) )
	{
		if ( errno != EINVAL && errno != EFBIG && errno != EINPROGRESS )
			err(1, "aio_error != EINVAL");
	}
	else
		errx(1, "aio_error did not fail");
	const struct aiocb* aiop = &aio;
	if ( aio_suspend(&aiop, 1, NULL) < 0 )
		err(1, "aio_suspend");
	if ( (errno = aio_error(&aio)) )
	{
		if ( errno != EINVAL && errno != EFBIG )
			err(1, "aio_error != EINVAL");
	}
	else
		errx(1, "aio_error did not fail");
	ssize_t ret = aio_return(&aio);
	if ( ret != -1 )
		errx(1, "aio_return() != -1");
	return 0;
}
