#ifdef __HAIKU__
#define _BSD_SOURCE
#endif

#include <sys/socket.h>

#if defined(__FreeBSD__) || defined(__NetBSD__) || defined(__DragonFly__) || defined(__minix__)
#include <sys/endian.h>
#elif defined(__APPLE__) || defined(_AIX) || defined(__sun__)
#define htobe16 htons
#define htobe32 htonl
#else
#include <endian.h>
#endif
#include <errno.h>
#if !defined(__sortix__) && !defined(_AIX) && !defined(__redox__)
#include <ifaddrs.h>
#endif
#include <fcntl.h>
#include <netdb.h>
#include <net/if.h>
#include <netinet/in.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#if defined(_AIX) && !defined(MSG_DONTWAIT)
#define MSG_DONTWAIT MSG_NONBLOCK
#endif

// Address to send packets too that do not send back any ICMP connection refused
// packets.
#define BLACKHOLE_HOST 0x08080808
#define BLACKHOLE_PORT 53

#include "../misc/errors.h"

__attribute__((unused))
static void sigpipe(int signum)
{
	(void) signum;
	int errnum = errno;
	printf("SIGPIPE\n");
	fflush(stdout);
	errno = errnum;
}

__attribute__((unused))
static in_addr_t subnet_mask_of(in_addr_t address)
{
#if !defined(__sortix__) && !defined(_AIX) && !defined(__redox__)
	in_addr_t result = 0;
	struct ifaddrs* ifa;
	if ( getifaddrs(&ifa) < 0 )
		test_err(1, "getifaddrs");
	for ( struct ifaddrs* iter = ifa; iter; iter = iter->ifa_next )
	{
		if ( iter->ifa_addr && iter->ifa_addr->sa_family == AF_INET )
		{
			in_addr_t addr =
				((struct sockaddr_in*) iter->ifa_addr)->sin_addr.s_addr;
			in_addr_t net =
				((struct sockaddr_in*) iter->ifa_netmask)->sin_addr.s_addr;
			if ( (addr & ~net) == (address & ~net) )
				result = net;
		}
	}
	freeifaddrs(ifa);
	return result;
#else
	// TODO: Implement getifaddrs in Sortix.
	address = ntohl(address);
	if ( (address & 0xFFFFFF00) == 0x0A000200 )
		return htonl(0xFFFFFF00); // 10.0.2.0/24
	else if ( (address & 0xFF000000) == 0x0A000000 )
		return htonl(0xFFFFF000); // 10.0.0.0/20
	else if ( (address & 0xFFF00000) == 0xAC100000 )
		return htonl(0xFFF00000); // 172.16.0.0/12
	else if ( (address & 0xFFFF0000) == 0xC0A80000 )
		return htonl(0xFFFFFF00); // 192.168.0.0/24
	else if ( (address & 0xFFFFFFC0) == 0x5863F400 )
		return htonl(0xFFFFFFC0); // 88.99.244.0/22
	else if ( (address & 0xFFFFFF00) == 0x8CD30900 )
		return htonl(0xFFFFFF00); // 140.211.9.0/24
	else
		return 0;
#endif
}

__attribute__((unused))
static int is_on_lan(in_addr_t address)
{
#if !defined(__sortix__) && !defined(_AIX) && !defined(__redox__)
	int result = 0;
	struct ifaddrs* ifa;
	if ( getifaddrs(&ifa) < 0 )
		test_err(1, "getifaddrs");
	for ( struct ifaddrs* iter = ifa; iter; iter = iter->ifa_next )
	{
		if ( iter->ifa_addr && iter->ifa_addr->sa_family == AF_INET )
		{
			in_addr_t addr =
				((struct sockaddr_in*) iter->ifa_addr)->sin_addr.s_addr;
			in_addr_t net =
				((struct sockaddr_in*) iter->ifa_netmask)->sin_addr.s_addr;
			if ( addr == htonl(INADDR_LOOPBACK) )
				continue;
			if ( (addr & ~net) == (address & ~net) )
				result = 1;
		}
	}
	freeifaddrs(ifa);
	return result;
#else
	// TODO: Implement getifaddrs in Sortix.
	return subnet_mask_of(address) != 0;
#endif
}
