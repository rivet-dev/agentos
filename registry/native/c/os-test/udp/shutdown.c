/* Test shutdown for read and write on a freshly made socket. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	if ( shutdown(fd, SHUT_RDWR) )
		err(1, "shutdown");
	return 0;
}
