/* Test whether a basic getnameinfo invocation works. */

#include <sys/socket.h>

#include <netdb.h>
#include <netinet/in.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	struct sockaddr_in in =
	{
		.sin_family = AF_INET,
		.sin_addr.s_addr = htonl(0x7F000001),
		.sin_port = htons(42),
	};
	char ip[INET_ADDRSTRLEN];
	char port[6];
	int ret = getnameinfo((struct sockaddr*) &in, sizeof(in), ip, sizeof(ip),
	                      port, sizeof(port), NI_NUMERICHOST | NI_NUMERICSERV);
	if ( ret )
		err(1, "getnameinfo: %s", gai_strerror(ret));
	if ( strcmp(ip, "127.0.0.1") != 0 )
		errx(1, "getnameinfo gave ip %s instead of 127.0.0.1", ip);
	if ( strcmp(port, "42") != 0 )
		errx(1, "getnameinfo gave port %s instead of 42", port);
	return 0;
}
