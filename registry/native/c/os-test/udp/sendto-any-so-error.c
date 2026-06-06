/* Test sending a datagram to the any address. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(INADDR_ANY);
	sin.sin_port = htobe16(65535);
	char x = 'x';
	if ( sendto(fd, &x, sizeof(x), 0,
	            (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "sendto");
	usleep(50000);
	int errnum;
	socklen_t errnumlen = sizeof(errnum);
	if ( getsockopt(fd, SOL_SOCKET, SO_ERROR, &errnum, &errnumlen) < 0 )
		err(1, "getsockopt: SO_ERROR");
	if ( errnum )
	{
		errno = errnum;
		err(1, "SO_ERROR");
	}
	return 0;
}
