/* Shut down for reading and then test receiving a datagram. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	if ( shutdown(fd, SHUT_RD) )
		err(1, "shutdown");
	char x;
	ssize_t amount = recv(fd, &x, sizeof(x), MSG_DONTWAIT);
	if ( amount < 0 )
		err(1, "recv");
	else if ( amount == 0 )
		puts("EOF");
	return 0;
}
