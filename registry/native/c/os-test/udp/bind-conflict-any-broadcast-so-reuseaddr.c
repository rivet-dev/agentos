/* Test whether binding to the same port on the any address and broadcast
   address will conflict when SO_REUSEADDR is passed on the second socket. */

#include "udp.h"

int main(void)
{
	int fd1 = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd1 < 0 )
		err(1, "first socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(INADDR_ANY);
	sin.sin_port = htobe16(0);
	if ( bind(fd1, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "first bind");
	struct sockaddr_in cos;
	socklen_t coslen = sizeof(cos);
	if ( getsockname(fd1, (struct sockaddr*) &cos, &coslen) < 0 )
		err(1, "getsockname");
	cos.sin_addr.s_addr = htobe32(INADDR_BROADCAST);
	int fd2 = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd2 < 0 )
		err(1, "second socket");
	int enable = 1;
	if ( setsockopt(fd2, SOL_SOCKET, SO_REUSEADDR, &enable, sizeof(enable)) < 0 )
		err(1, "setsockopt: SO_REUSEADDR");
	if ( bind(fd2, (const struct sockaddr*) &cos, sizeof(cos)) < 0 )
		err(1, "second bind");
	return 0;
}
