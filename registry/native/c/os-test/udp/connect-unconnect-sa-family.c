/* Connect to loopback address port 65535, then test if unconnect works if the
   unconnect address is a sa_family_t. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(INADDR_LOOPBACK);
	sin.sin_port = htobe16(65535);
	if ( connect(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "first connect");
	sa_family_t family = AF_UNSPEC;
	if ( connect(fd, (const struct sockaddr*) &family, sizeof(family)) < 0 )
		err(1, "second connect");
	return 0;
}
