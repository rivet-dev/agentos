/* Test whether a basic getservbyport invocation works. */

#include <sys/socket.h>

#include <errno.h>
#include <netdb.h>
#include <netinet/in.h>
#include <stdbool.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	struct servent* entry = getservbyport(htons(80), "tcp");
	if ( !entry )
	{
		if ( errno )
			err(1, "getservbyport");
		errx(1, "port 80 (http) was not found for tcp");
	}
	if ( entry->s_port != htons(80) )
		errx(1, "http was not port 80");
	if ( strcmp(entry->s_proto, "tcp") != 0 )
		errx(1, "http was not on protocol tcp");
	bool found = !strcmp(entry->s_name, "http");
	for ( size_t i = 0; entry->s_aliases[i]; i++ )
		if ( !strcmp(entry->s_aliases[i], "http") )
			found = true;
	if ( !found )
		errx(1, "found service was not named http");
	return 0;
}
