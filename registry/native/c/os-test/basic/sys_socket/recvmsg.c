/* Test whether a basic recvmsg invocation works. */

#include <sys/socket.h>

#include <netinet/in.h>

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
	struct iovec iov;
	struct msghdr msg;
	memset(&iov, 0, sizeof(iov));
	iov.iov_base = &c;
	iov.iov_len = 1;
	memset(&msg, 0, sizeof(msg));
	msg.msg_name = (struct sockaddr*) &name;
	msg.msg_namelen = sizeof(name);
	msg.msg_iov = &iov;
	msg.msg_iovlen = 1;
	if ( sendmsg(server_fd, &msg, 0) != 1 )
		err(1, "sendmsg");
	return 0;
}
