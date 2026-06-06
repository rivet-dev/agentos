/* Test if listen fails with ENOTSUP. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	if ( listen(fd, 1) < 0 )
		err(1, "listen");
	return 0;
}
