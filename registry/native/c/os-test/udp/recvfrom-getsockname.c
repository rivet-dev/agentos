/* Receive a datagram on a freshly made socket and then test the local
   address. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr_in sin;
	socklen_t sinlen = sizeof(sin);
	char x;
	if ( recvfrom(fd, &x, sizeof(x), MSG_DONTWAIT,
	              (struct sockaddr*) &sin, &sinlen) < 0 )
	{
		if ( errno != EAGAIN && errno != EWOULDBLOCK )
			err(1, "recvfrom");
	}
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getsockname(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "getsockname");
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
