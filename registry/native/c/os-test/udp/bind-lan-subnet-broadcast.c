/* Test binding to the broadcast address in the lan subnet. */

#include "udp.h"

int main(void)
{
	int fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd < 0 )
		err(1, "first socket");
	struct sockaddr_in sin;
	memset(&sin, 0, sizeof(sin));
	sin.sin_family = AF_INET;
	sin.sin_addr.s_addr = htobe32(BLACKHOLE_HOST);
	sin.sin_port = htobe16(BLACKHOLE_PORT);
	if ( connect(fd, (const struct sockaddr*) &sin, sizeof(sin)) < 0 )
		err(1, "connect");
	struct sockaddr_in local;
	socklen_t locallen = sizeof(local);
	if ( getsockname(fd, (struct sockaddr*) &local, &locallen) < 0 )
		err(1, "getsockname");
	in_addr_t address = local.sin_addr.s_addr;
	in_addr_t subnetmask;
	if ( !(subnetmask = subnet_mask_of(address)) )
		errx(1, "couldn't deduce local area subnet of: %u.%u.%u.%u",
		     address >>  0 & 0xFF, address >>  8 & 0xFF,
		     address >> 16 & 0xFF, address >> 24 & 0xFF);
	in_addr_t target_address = address | ~subnetmask;
	int fd2 = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
	if ( fd2 < 0 )
		err(1, "second socket");
	struct sockaddr_in cos;
	memset(&sin, 0, sizeof(sin));
	cos.sin_family = AF_INET;
	cos.sin_addr.s_addr = target_address;
	cos.sin_port = htobe16(0);
	if ( bind(fd2, (const struct sockaddr*) &cos, sizeof(cos)) < 0 )
		err(1, "bind");
	return 0;
}
