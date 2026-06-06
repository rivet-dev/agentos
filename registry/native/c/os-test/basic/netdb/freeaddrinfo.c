/* Test whether a basic freeaddrinfo invocation works. */

#include <netdb.h>

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
	freeaddrinfo(res0);
	return 0;
}
