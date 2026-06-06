/* Test whether a basic bind invocation works. */

#include <sys/socket.h>

#include <arpa/inet.h>
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
	return 0;
}
