/* Test whether a basic FD_SET invocation works. */

#include <sys/select.h>

#include "../basic.h"

int main(void)
{
	fd_set fdset;
	FD_ZERO(&fdset);
	int fd = 0;
	if ( FD_ISSET(fd, &fdset) )
		errx(1, "FD_ZERO did not zero");
	FD_SET(fd, &fdset);
	if ( !FD_ISSET(fd, &fdset) )
		errx(1, "FD_SET did not set");
	return 0;
}
