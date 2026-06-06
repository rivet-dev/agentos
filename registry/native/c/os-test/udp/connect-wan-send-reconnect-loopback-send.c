/* Connect to a public internet address, send a datagram, then testing
   reconnecting to the loopback address port 65535. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(BLACKHOLE_HOST);
	sin.sin_port = htobe16(BLACKHOLE_PORT);
	if ( connect(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "first connect");
	char x = 'x';
	if ( send(fd, &x, sizeof(x), 0) < 0 )
		err(1, "first send");
	struct sockaddr_in cos;
	memset(&cos, 0, sizeof(cos));
	cos.sin_family = AF_INET;
	cos.sin_addr.s_addr = htobe32(INADDR_LOOPBACK);
	cos.sin_port = htobe16(65535);
	if ( connect(fd, (const struct sockaddr*) &cos, sizeof(cos)) < 0 )
		err(1, "second connect");
	char y = 'y';
	if ( send(fd, &y, sizeof(y), 0) < 0 )
		err(1, "second send");
	return 0;
}
