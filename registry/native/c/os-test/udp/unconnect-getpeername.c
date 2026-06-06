/* Unconnect a freshly made socket and test its remote address. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr sin;
	memset(&sin, 0, sizeof(sin));
	sin.sa_family = AF_UNSPEC;
	if ( connect(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "unconnect");
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getpeername(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "getpeername");
	char host[INET_ADDRSTRLEN + 1];
	char port[5 + 1];
	getnameinfo((const struct sockaddr*) &local, locallen, host, sizeof(host),
	            port, sizeof(port), NI_NUMERICHOST | NI_NUMERICSERV);
	printf("%s:%s\n", host, port);
	return 0;
}
