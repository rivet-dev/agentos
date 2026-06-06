/* Test whether a basic getnetbyname invocation works. */

#include <netdb.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	// There are no standardized networks, so see if one is configured, and then
	// try to look it up.
	struct netent* entry = getnetent();
	if ( entry )
	{
		uint32_t net = entry->n_net;
		int type = entry->n_addrtype;
		const char* name = strdup(entry->n_name);
		if ( !name )
			errx(1, "strdup");
		errno = 0;
		entry = getnetbyname(name);
		if ( !entry )
		{
			if ( errno )
				err(1, "getnetbyname");
			errx(1, "getnetbyname unexpectedly found nothing");
		}
		if ( strcmp(name, entry->n_name) )
			errx(1, "getnetbyaddr found wrong name");
		if ( entry->n_net != net )
			errx(1, "getnetbyaddr found wrong net");
		if ( entry->n_addrtype != type )
			errx(1, "getnetbyaddr found wrong type");
	}
	// Otherwise test that a lookup will find nothing.
	else
	{
		const char* name = "loopback";
		if ( getnetbyname(name) )
			errx(1, "getnetbyname succeded unexpectedly");
	}
	return 0;
}
