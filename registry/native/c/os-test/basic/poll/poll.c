/* Test whether a basic poll invocation works. */

#include <poll.h>
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
	struct pollfd pfds[2] =
	{
		{ .fd = fds[0], .events = POLLIN },
		{ .fd = fds[1], .events = POLLOUT },
	};
	while ( true )
	{
		int ret = poll(pfds, 2, -1);
		if ( ret < 0 )
			err(1, "poll");
		if ( ret == 0 )
			errx(1, "poll() == 0");
		if ( 2 < ret )
			errx(1, "2 < poll()");
		if ( full && count_sent == count_recv )
		{
			if ( pfds[0].revents & POLLIN )
				err(1, "pipe was readable when empty");
			if ( !(pfds[1].revents & POLLOUT ))
				err(1, "pipe was non-writable when empty");
			if ( ret != 1 )
				errx(1, "poll() != 1");
			break;
		}
		if ( !full )
		{
			if ( pfds[1].revents & POLLOUT )
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
			if ( pfds[0].revents & POLLIN )
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
