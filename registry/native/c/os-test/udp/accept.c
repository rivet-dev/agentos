/* Test if accept on UDP socket is rejected with ENOTSUP. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	if ( accept(fd, NULL, NULL) < 0 )
		err(1, "accept");
	return 0;
}
