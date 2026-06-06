/* Test whether a basic shutdown invocation works. */

#include <sys/socket.h>

#include <arpa/inet.h>
#include <fcntl.h>
#include <netinet/in.h>

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
	int server_fd = accept(listen_fd, NULL, NULL);
	if ( server_fd < 0 )
		err(1, "accept4");
	if ( shutdown(server_fd, SHUT_WR) < 0 )
		err(1, "shutdown");
	char x = 'y';
	ssize_t amount = recv(client_fd, &x, 1, 0);
	if ( amount < 0 )
		err(1, "recv");
	if ( amount != 0 )
		errx(1, "recv did not get EOF");
	return 0;
}
