/* Test unconnecting a freshly made socket. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr sin;
	memset(&sin, 0, sizeof(sin));
	sin.sa_family = AF_UNSPEC;
	if ( connect(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "unconnect");
	return 0;
}
