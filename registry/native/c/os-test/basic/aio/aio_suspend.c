/* Test whether a basic aio_suspend invocation works. */

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
	int fd = fileno(fp);
	char buffer[6] = {'F', 'O', 'O', 'B', 'A', 'R'};
	struct aiocb aio1 =
	{
		.aio_fildes = fd,
		.aio_offset = 0,
		.aio_buf = buffer,
		.aio_nbytes = sizeof(buffer),
		.aio_sigevent = { .sigev_notify = SIGEV_NONE },
	};
	if ( aio_write(&aio1) < 0 )
		err(1, "first aio_write");
	struct aiocb aio2 =
	{
		.aio_fildes = fd,
		.aio_offset = 6,
		.aio_buf = buffer,
		.aio_nbytes = sizeof(buffer),
		.aio_sigevent = { .sigev_notify = SIGEV_NONE },
	};
	if ( aio_write(&aio2) < 0 )
		err(1, "second aio_write");
	const struct aiocb* const aiop[2] = { &aio1, &aio2 };
	if ( aio_suspend(aiop, 2, NULL) < 0 )
		err(1, "aio_suspend");
	int done = 0;
	for ( int i = 0; i < 2; i++ )
	{
		if ( (errno = aio_error((struct aiocb*) aiop[i])) )
		{
			if ( errno == EINPROGRESS )
				continue;
			err(1, "aio_error");
		}
		ssize_t ret = aio_return((struct aiocb*) aiop[i]);
		if ( ret < 0 )
			errx(1, "aio_return() != < 0");
		if ( ret != sizeof(buffer) )
			errx(1, "aio_return() != sizeof(buffer)");
		done++;
	}
	if ( !done )
		errx(1, "no async io had completed");
	return 0;
}
