/* Test whether a freshly made socket is bound to a device according to
   SO_BINDTODEVICE. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
#ifdef SO_BINDTODEVICE
	char ifname[IF_NAMESIZE + 1];
	socklen_t ifnamelen = sizeof(ifname);
	if ( getsockopt(fd, SOL_SOCKET, SO_BINDTODEVICE, ifname, &ifnamelen) < 0 )
		err(1, "getsockopt: SO_BINDTODEVICE");
	ifname[ifnamelen] = '\0';
	puts(ifname);
#else
	errno = ENOSYS;
	err(1, "getsockopt: SO_BINDTODEVICE");
#endif
	return 0;
}
