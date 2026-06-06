/* Test that a socket being non-blocking has no effect on accept failing with
   ENOTSUP. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	if ( fcntl(fd, F_SETFL, O_NONBLOCK) < 0 )
		err(1, "fcntl");
	if ( accept(fd, NULL, NULL) < 0 )
		err(1, "accept");
	return 0;
}
