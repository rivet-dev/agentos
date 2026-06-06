/* Test whether a basic FD_CLR invocation works. */

#include <sys/select.h>

#include "../basic.h"

int main(void)
{
	fd_set fdset;
	FD_ZERO(&fdset);
	int fd = 0;
	FD_SET(fd, &fdset);
	if ( !FD_ISSET(fd, &fdset) )
		errx(1, "FD_SET did not set");
	FD_CLR(fd, &fdset);
	if ( FD_ISSET(fd, &fdset) )
		errx(1, "FD_CLR did not clear");
	return 0;
}
