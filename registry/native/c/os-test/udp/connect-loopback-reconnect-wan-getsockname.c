/* Connect to the loopback address port 65535, and then test reconnecting to the
   public internet and print the local address. */

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
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getsockname(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "first getsockname");
	char host[INET_ADDRSTRLEN + 1];
	char port[5 + 1];
	getnameinfo((const struct sockaddr*) &local, locallen, host, sizeof(host),
	            port, sizeof(port), NI_NUMERICHOST | NI_NUMERICSERV);
	struct sockaddr_in cos;
	memset(&sin, 0, sizeof(cos));
	cos.sin_family = AF_INET;
	cos.sin_addr.s_addr = htobe32(BLACKHOLE_HOST);
	cos.sin_port = htobe16(BLACKHOLE_PORT);
	if ( connect(fd, (const struct sockaddr*) &cos, sizeof(cos)) < 0 )
		err(1, "second connect");
	if ( getsockname(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "second getsockname");
	char second_port[5 + 1];
	getnameinfo((const struct sockaddr*) &local, locallen, host, sizeof(host),
	            second_port, sizeof(second_port),
	            NI_NUMERICHOST | NI_NUMERICSERV);
	if ( is_on_lan(local.sin_addr.s_addr) )
		printf("192.168.1.x");
	else
		printf("%s", host);
	printf(":");
	if ( !strcmp(port, second_port) )
		printf("same port");
	else
		printf("%s", second_port);
	printf("\n");
	return 0;
}
