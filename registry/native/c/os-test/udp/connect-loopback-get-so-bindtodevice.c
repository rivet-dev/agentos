/* Connect to the loopback address port 65535 and test if the socket was bound
   to an interface according to SO_BINDTODEVICE. */

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
		err(1, "connect");
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
