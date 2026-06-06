/* Shut down for reading and writing and then test sending a datagram. */

#include "udp.h"

int main(void)
{
	signal(SIGPIPE, sigpipe);
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	if ( shutdown(fd, SHUT_RDWR) )
		err(1, "shutdown");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(BLACKHOLE_HOST);
	sin.sin_port = htobe16(BLACKHOLE_PORT);
	char x = 'x';
	if ( sendto(fd, &x, sizeof(x), 0,
	            (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "sendto");
	return 0;
}
