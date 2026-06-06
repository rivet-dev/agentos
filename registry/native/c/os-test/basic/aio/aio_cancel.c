/* Test whether a basic aio_cancel invocation works. */

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
	char buffer[3] = {'F', 'O', 'O'};
	struct aiocb aio =
	{
		.aio_fildes = fd,
		.aio_offset = 1,
		.aio_buf = buffer,
		.aio_nbytes = sizeof(buffer),
		.aio_sigevent = { .sigev_notify = SIGEV_NONE },
	};
	if ( aio_write(&aio) < 0 )
		err(1, "aio_write");
	int ret = aio_cancel(fd, &aio);
	if ( ret < 0 )
		err(1, "aio_cancel");
	if ( ret != AIO_CANCELED && ret != AIO_NOTCANCELED && ret != AIO_ALLDONE )
		errx(1, "aio_cancel returned weird value");
	if ( ret == AIO_NOTCANCELED )
	{
		const struct aiocb* aiop = &aio;
		if ( aio_suspend(&aiop, 1, NULL) < 0 )
			err(1, "aio_suspend");
	}
	if ( (errno = aio_error(&aio)) )
	{
		if ( ret == AIO_CANCELED )
		{
			if ( errno != ECANCELED )
				err(1, "aio_error() != ECANCELED");
		}
		else
			err(1, "aio_error");
	}
	else if ( ret == AIO_CANCELED )
		errx(1, "aio_error() != ECANCELED");
	if ( ret != AIO_CANCELED )
	{
		ssize_t ret = aio_return(&aio);
		if ( ret < 0 )
			errx(1, "aio_return() < 0");
		if ( ret != sizeof(buffer) )
			errx(1, "aio_return() != sizeof(buffer)");
	}
	char check[16];
	if ( pread(fd, check, sizeof(check), 0) != 6 )
		errx(1, "pread() != 6");
	if ( ret == AIO_CANCELED && memcmp(check, "foobar", 6) != 0 )
		errx(1, "pread did not read \"foobar\"");
	if ( ret != AIO_CANCELED && memcmp(check, "fFOOar", 6) != 0 )
		errx(1, "pread did not read \"fFOOar\"");
	return 0;
}
