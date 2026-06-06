/* Test whether a basic select invocation works. */

#include <sys/select.h>

#include <stdbool.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
#ifdef __minix__
	alarm(1);
#endif
	// Fill a pipe buffer and empty the pipe, one byte at a time.
	int fds[2];
	if ( pipe(fds) < 0 )
		err(1, "pipe");
	size_t count_sent = 0;
	size_t count_recv = 0;
	bool full = false;
	if ( FD_SETSIZE <= fds[0] )
		errx(1, "FD_SETSIZE <= fds[0]");
	if ( FD_SETSIZE <= fds[1] )
		errx(1, "FD_SETSIZE <= fds[1]");
	int max = fds[0] > fds[1] ? fds[0] : fds[1];
	fd_set read_set, write_set, error_set;
	while ( true )
	{
		FD_ZERO(&read_set);
		FD_ZERO(&write_set);
		FD_ZERO(&error_set);
		FD_SET(fds[0], &read_set);
		FD_SET(fds[0], &error_set);
		FD_SET(fds[1], &write_set);
		FD_SET(fds[1], &error_set);
		int ret = select(max + 1, &read_set, &write_set, &error_set, NULL);
		if ( ret < 0 )
			err(1, "select");
		if ( ret == 0 )
			errx(1, "select() == 0");
		if ( 2 < ret )
			errx(1, "2 < select()");
		if ( full && count_sent == count_recv )
		{
			if ( FD_ISSET(fds[0], &read_set) )
				err(1, "pipe was readable when empty");
			if ( !FD_ISSET(fds[1], &write_set) )
				err(1, "pipe was non-writable when empty");
			if ( ret != 1 )
				errx(1, "select() != 1");
			break;
		}
		if ( !full )
		{
			if ( FD_ISSET(fds[1], &write_set) )
			{
				if ( write(fds[1], "x", 1) != 1 )
					err(1, "write");
				count_sent++;
			}
			else
			{
				if ( !count_sent )
					errx(1, "pipe was non-writable when empty");
				full = true;
			}
		}
		if ( full )
		{
			if ( FD_ISSET(fds[0], &read_set) )
			{
				char c;
				if ( read(fds[0], &c, 1) != 1 )
					err(1, "read");
				count_recv++;
			}
			else
				errx(1, "pipe was non-readable when non-empty");
		}
	}
	return 0;
}
