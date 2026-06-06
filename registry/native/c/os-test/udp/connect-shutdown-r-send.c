/* Connect to loopback address port 65535, shutdown for reading, and then test
   sending a datagram. */

#include "udp.h"

int main(void)
{
	signal(SIGPIPE, sigpipe);
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
	if ( shutdown(fd, SHUT_RD) )
		err(1, "shutdown");
	char x = 'x';
	if ( send(fd, &x, sizeof(x), 0) < 0 )
		err(1, "send");
	return 0;
}
