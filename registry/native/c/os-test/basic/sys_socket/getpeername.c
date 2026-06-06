/* Test whether a basic getpeername invocation works. */

#include <sys/socket.h>

#include <arpa/inet.h>
#include <netinet/in.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	int listen_fd = socket(AF_INET, SOCK_STREAM, 0);
	if ( listen_fd < 0 )
		err(1, "socket");
	struct sockaddr_in addr =
	{
		.sin_family = AF_INET,
		.sin_addr = { .s_addr = htonl(0x7F000001 /* 127.0.0.1 */) },
		.sin_port = htons(0),
	};
	if ( bind(listen_fd, (const struct sockaddr*) &addr, sizeof(addr)) < 0 )
		err(1, "bind");
	if ( listen(listen_fd, 1) < 0 )
		err(1, "listen");
	socklen_t addr_len = sizeof(struct sockaddr_in);
	if ( getsockname(listen_fd, (struct sockaddr*) &addr, &addr_len) < 0 )
		err(1, "getsockname");
	if ( addr_len != sizeof(struct sockaddr_in) )
		errx(1, "getsockname returned odd length");
	int client_fd = socket(AF_INET, SOCK_STREAM, 0);
	if ( client_fd < 0 )
		err(1, "client socket");
	if ( connect(client_fd, (const struct sockaddr*) &addr, sizeof(addr)) < 0 )
		err(1, "connect");
	struct sockaddr_in peer;
	socklen_t peer_len = sizeof(struct sockaddr_in);
	if ( getpeername(client_fd, (struct sockaddr*) &peer, &peer_len) < 0 )
		err(1, "getpeername");
	if ( peer_len != sizeof(struct sockaddr_in) )
		errx(1, "getpeername returned odd length");
	if ( memcmp(&addr, &peer, sizeof(struct sockaddr_in)) != 0 )
		errx(1, "getpeername gave wrong address");
	return 0;
}
