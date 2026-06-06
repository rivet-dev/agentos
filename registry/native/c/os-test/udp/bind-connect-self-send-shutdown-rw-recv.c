/* Test binding on any address port 0, use getsockname to bind the address
   actually bound to, connect to itself, send a datagram, shutdown for reading
   and writing, and then test receiving a datagram. */

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
	if ( connect(fd, (const struct sockaddr*) &local, locallen) < 0 )
		err(1, "connect");
	char x = 'x';
	if ( send(fd, &x, sizeof(x), 0) < 0 )
		err(1, "send");
	usleep(50000);
	if ( shutdown(fd, SHUT_RDWR) )
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
