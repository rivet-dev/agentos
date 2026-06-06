/* Connect to a public internet address, unconnect, then test rebinding to the
   any address port 0 and printing the local address. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(BLACKHOLE_HOST);
	sin.sin_port = htobe16(BLACKHOLE_PORT);
	if ( connect(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "first connect");
	struct sockaddr_in assigned;
	socklen_t assignedlen = sizeof(assigned);
	if ( getsockname(fd, (struct sockaddr*) &assigned, &assignedlen) < 0 )
		err(1, "getsockname");
	struct sockaddr cos;
	memset(&cos, 0, sizeof(cos));
	cos.sa_family = AF_UNSPEC;
	if ( connect(fd, (const struct sockaddr*) &cos, sizeof(cos)) < 0 )
		err(1, "second connect");
	if ( bind(fd, (const struct sockaddr*) &assigned, sizeof(assigned)) < 0 )
		err(1, "bind");
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getsockname(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "second getsockname");
	char host[INET_ADDRSTRLEN + 1];
	char port[5 + 1];
	getnameinfo((const struct sockaddr*) &local, locallen, host, sizeof(host),
	            port, sizeof(port), NI_NUMERICHOST | NI_NUMERICSERV);
	if ( is_on_lan(local.sin_addr.s_addr) )
		printf("192.168.1.x");
	else
		printf("%s", host);
	printf(":");
	if ( !strcmp(port, "0") )
		printf("%s", port);
	else
		printf("non-zero");
	printf("\n");
	return 0;
}
