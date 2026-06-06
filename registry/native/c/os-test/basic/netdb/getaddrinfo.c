/* Test whether a basic getaddrinfo invocation works. */

#include <sys/socket.h>

#include <netdb.h>
#include <netinet/in.h>
#include <stdbool.h>

#include "../basic.h"

int main(void)
{
	struct addrinfo hints = { .ai_flags = AI_PASSIVE };
	struct addrinfo* res0;
	int ret = getaddrinfo("localhost", NULL, &hints, &res0);
	if ( ret )
		errx(1, "getaddrinfo: localhost: %s", gai_strerror(ret));
	if ( !res0 )
		errx(1, "getaddrinfo gave NULL");
	bool found_ipv4 = false;
	bool found_ipv6 = false;
	for ( struct addrinfo* res = res0; res; res = res->ai_next )
	{
		if ( res->ai_family == AF_INET )
		{
			struct sockaddr_in* in = (struct sockaddr_in*) res->ai_addr;
			if ( in->sin_family != AF_INET )
				errx(1, "AF_INET address had wrong family");
			if ( in->sin_addr.s_addr != htonl(0x7F000001) /* 127.0.0.1 */ )
				errx(1, "AF_INET address was not 127.0.0.1");
			if ( in->sin_port != htons(0) )
				errx(1, "AF_INET port was not 0");
			found_ipv4 = true;
		}
		else if ( res->ai_family == AF_INET6 )
		{
			struct sockaddr_in6* in6 = (struct sockaddr_in6*) res->ai_addr;
			found_ipv6 = true;
			if ( in6->sin6_family != AF_INET6 )
				errx(1, "AF_INET6 address had wrong family");
			if ( memcmp(&in6->sin6_addr, &in6addr_loopback,
			            sizeof(struct in6_addr)) != 0 )
				errx(1, "AF_INET6 address was not ::1");
			if ( in6->sin6_port != htons(0) )
				errx(1, "AF_INET6 port was not 0");
		}
	}
	if ( !found_ipv4 && !found_ipv6 )
		errx(1, "getaddrinfo returned neither IPv4 nor IPv6");
	return 0;
}
