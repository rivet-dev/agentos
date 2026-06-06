/* Bind to loopback port 0, make another socket, shutdown the first socket for
   writing, send a datagram from the second socket to the first socket, and
   then test receiving a datagram on the first socket. */

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
	sin.sin_port = htobe16(0);
	if ( bind(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "bind");
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getsockname(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "getsockname");
	int fd2 = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd2 < 0 )
		err(1, "socket");
	if ( shutdown(fd, SHUT_WR) < 0 )
		err(1, "shutdown");
	char x = 'x';
	if ( sendto(fd2, &x, sizeof(x), 0,
	            (const struct sockaddr*) &local, locallen) < 0 )
		err(1, "sendto");
	usleep(50000);
	ssize_t amount = recv(fd, &x, sizeof(x), MSG_DONTWAIT);
	if ( amount < 0 )
		err(1, "recv");
	else if ( amount == 0 )
		puts("EOF");
	else if ( amount != 1 )
		printf("recv %zi bytes\n", amount);
	else if ( x != 'x' )
		printf("recv wrong byte");
	else
		printf("%c\n", x);
	return 0;
}
