/* Test whether a basic recvfrom invocation works. */

#include <sys/socket.h>

#include <netinet/in.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	int server_fd = socket(AF_INET, SOCK_DGRAM, 0);
	if ( server_fd < 0 )
		err(1, "socket");
	struct sockaddr_in addr =
	{
		.sin_family = AF_INET,
		.sin_addr = { .s_addr = htonl(0x7F000001 /* 127.0.0.1 */) },
		.sin_port = htons(0),
	};
	if ( bind(server_fd, (const struct sockaddr*) &addr, sizeof(addr)) < 0 )
		err(1, "bind");
	socklen_t addr_len = sizeof(struct sockaddr_in);
	if ( getsockname(server_fd, (struct sockaddr*) &addr, &addr_len) < 0 )
		err(1, "getsockname");
	if ( addr_len != sizeof(struct sockaddr_in) )
		errx(1, "getsockname returned odd length");
	int client_fd = socket(AF_INET, SOCK_DGRAM, 0);
	if ( client_fd < 0 )
		err(1, "client socket");
	if ( connect(client_fd, (const struct sockaddr*) &addr, sizeof(addr)) < 0 )
		err(1, "connect");
	struct sockaddr_in name;
	socklen_t name_len = sizeof(struct sockaddr_in);
	if ( getsockname(client_fd, (struct sockaddr*) &name, &name_len) < 0 )
		err(1, "getsockname");
	if ( addr_len != sizeof(struct sockaddr_in) )
		errx(1, "getsockname returned odd length");
	char c = 'x';
	if ( sendto(server_fd, &c, 1, 0, (const struct sockaddr*) &name,
	            sizeof(name)) != 1 )
		err(1, "sendto");
	struct sockaddr_in from;
	socklen_t from_len = sizeof(struct sockaddr_in);
	char x = 'y';
	ssize_t amount = recvfrom(client_fd, &x, 1, 0, (struct sockaddr*) &from,
	                          &from_len);
	if ( amount < 0 )
		err(1, "recvfrom");
	if ( from_len != sizeof(struct sockaddr_in) )
		errx(1, "recvfrom returned odd length");
	if ( memcmp(&addr, &from, sizeof(struct sockaddr_in)) != 0 )
		errx(1, "received from wrong address");
	if ( amount != 1 )
		errx(1, "recvfrom did not get one byte");
	if ( c != x )
		errx(1, "received %c instead of %c\n", x, c);
	return 0;
}
