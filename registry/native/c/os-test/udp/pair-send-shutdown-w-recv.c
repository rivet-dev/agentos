/* Create two loopback address sockets connected to each other, send a datagram
   from the second socket to the first socket, shutdown the first socket for
   writing, and then test receiving a datagram on the first socket. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "first socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(INADDR_LOOPBACK);
	sin.sin_port = htobe16(0);
	if ( bind(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "first bind");
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getsockname(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "first getsockname");
	int fd2 = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd2 < 0 )
		err(1, "second socket");
	if ( bind(fd2, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "second bind");
	struct sockaddr_in local2;
	socklen_t locallen2 = sizeof(local2);
	if ( getsockname(fd2, (struct sockaddr*) &local2, &locallen2) < 0 )
		err(1, "second getsockname");
	if ( connect(fd, (const struct sockaddr*) &local2, locallen2) < 0 )
		err(1, "first connect");
	if ( connect(fd2, (const struct sockaddr*) &local, locallen) < 0 )
		err(1, "second connect");
	char x = 'x';
	if ( send(fd2, &x, sizeof(x), 0) < 0 )
		err(1, "send");
	usleep(50000);
	if ( shutdown(fd, SHUT_WR) < 0 )
		err(1, "shutdown");
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
