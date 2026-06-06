/* Test whether a basic getnetbyaddr invocation works. */

#include <sys/socket.h>

#include <errno.h>
#include <netdb.h>

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
		entry = getnetbyaddr(net, type);
		if ( !entry )
		{
			if ( errno )
				err(1, "getnetbyaddr");
			errx(1, "getnetbyaddr unexpectedly found nothing");
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
		uint32_t net = 0;
		int type = AF_INET;
		if ( getnetbyaddr(net, type) )
			errx(1, "getnetbyaddr succeded unexpectedly");
	}
	return 0;
}
