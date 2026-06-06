/* Test whether a basic lio_listio invocation works. */

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
	struct aiocb nop =
	{
		.aio_lio_opcode = LIO_NOP,
	};
	struct aiocb aio1 =
	{
		.aio_fildes = fd,
		.aio_offset = 0,
		.aio_buf = buffer,
		.aio_nbytes = sizeof(buffer),
		.aio_lio_opcode = LIO_WRITE,
	};
	struct aiocb aio2 =
	{
		.aio_fildes = fd,
		.aio_offset = 6,
		.aio_buf = buffer,
		.aio_nbytes = sizeof(buffer),
		.aio_lio_opcode = LIO_WRITE,
	};
	struct sigevent sev = { .sigev_notify = SIGEV_NONE };
	struct aiocb* aiop[3] = { &nop, &aio1, &aio2 };
	if ( lio_listio(LIO_WAIT, aiop, 3, &sev) < 0 )
		err(1, "lio_listio");
	int done = 0;
	for ( int i = 1; i < 3; i++ )
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
			errx(1, "aio_return() != sizeof(buffer (%zi))");
		done++;
	}
	if ( done != 2 )
		errx(1, "incomplete io");
	return 0;
}
