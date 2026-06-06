/* Connect to the loopback address port 65535 and then test receiving a
   datagram. */

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
		err(1, "connect");
	char x;
	ssize_t amount = recv(fd, &x, sizeof(x), MSG_DONTWAIT);
	if ( amount < 0 )
		err(1, "recv");
	if ( amount == 0 )
		puts("EOF");
	return 0;
}
