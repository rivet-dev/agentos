/* Connect to loopback address port 65535, send a datagram, and expect an ICMP
   connection refused packet, and then test if send delivers the asynchronous
   error. */

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
	char x = 'x';
	if ( send(fd, &x, sizeof(x), 0) < 0 )
		warn("first send");
	usleep(50000);
	if ( send(fd, &x, sizeof(x), 0) < 0 )
		warn("second send");
	else
		warnx("second send: no error");
	return 0;
}
