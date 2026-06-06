/* Test binding to the broadcast address port 0 and print the remote address. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(INADDR_BROADCAST);
	sin.sin_port = htobe16(0);
	if ( bind(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "bind");
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getpeername(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "getpeername");
	char host[INET_ADDRSTRLEN + 1];
	char port[5 + 1];
	getnameinfo((const struct sockaddr*) &local, locallen, host, sizeof(host),
	            port, sizeof(port), NI_NUMERICHOST | NI_NUMERICSERV);
	printf("%s:%s\n", port, port);
	return 0;
}
