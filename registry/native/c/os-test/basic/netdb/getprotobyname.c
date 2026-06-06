/* Test whether a basic getprotobyname invocation works. */

#include <errno.h>
#include <netdb.h>
#include <stdbool.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	struct protoent* entry = getprotobyname("tcp");
	if ( !entry )
	{
		if ( errno )
			err(1, "getprotobyname");
		errx(1, "tcp was not found");
	}
	if ( entry->p_proto != 6 )
		errx(1, "tcp was not protocol 6");
	bool found = !strcmp(entry->p_name, "tcp");
	for ( size_t i = 0; entry->p_aliases[i]; i++ )
		if ( !strcmp(entry->p_aliases[i], "tcp") )
			found = true;
	if ( !found )
		errx(1, "found protocol was not named tcp");
	return 0;
}
