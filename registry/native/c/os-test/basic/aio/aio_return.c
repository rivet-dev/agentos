/* Test whether a basic aio_return invocation works. */

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
	struct aiocb aio =
	{
		.aio_fildes = fd,
		.aio_offset = 0,
		.aio_buf = buffer,
		.aio_nbytes = sizeof(buffer),
		.aio_sigevent = { .sigev_notify = SIGEV_NONE },
	};
	if ( aio_write(&aio) < 0 )
		err(1, "aio_write");
	const struct aiocb* aiop = &aio;
	if ( aio_suspend(&aiop, 1, NULL) < 0 )
		err(1, "aio_suspend");
	if ( (errno = aio_error(&aio)) )
		err(1, "aio_error");
	ssize_t ret = aio_return(&aio);
	if ( ret < 0 )
		errx(1, "aio_return() < 0");
	if ( ret != sizeof(buffer) )
		errx(1, "aio_return() != sizeof(buffer)");
	return 0;
}
