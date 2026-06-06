/* Connect to the loopback address port 65535, then test reconnecting to the any
   address port 0 and print the local address. */

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
	sin.sin_port = htobe16(65535);
	if ( connect(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "first connect");
	struct sockaddr_in cos;
	memset(&cos, 0, sizeof(cos));
	cos.sin_family = AF_INET;
	cos.sin_addr.s_addr = htobe32(INADDR_ANY);
	cos.sin_port = htobe16(0);
	if ( connect(fd, (const struct sockaddr*) &cos, sizeof(cos)) < 0 )
		err(1, "second connect");
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
