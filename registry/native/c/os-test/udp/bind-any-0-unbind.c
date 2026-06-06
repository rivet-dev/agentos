/* Bind to the any address port 0 and test if binding to AF_UNSPEC unbinds the
   socket. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(INADDR_ANY);
	sin.sin_port = htobe16(0);
	if ( bind(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "bind");
	struct sockaddr_in cos;
	memset(&cos, 0, sizeof(cos));
	cos.sin_family = AF_UNSPEC;
	if ( bind(fd, (const struct sockaddr*) &cos, sizeof(cos)) < 0 )
		err(1, "bind AF_UNSPEC");
	return 0;
}
