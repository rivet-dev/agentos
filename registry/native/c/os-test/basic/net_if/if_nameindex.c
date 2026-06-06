/* Test whether a basic if_nameindex invocation works. */

#include <net/if.h>

#include "../basic.h"

int main(void)
{
	struct if_nameindex* index = if_nameindex();
	if ( !index )
		err(1, "if_nameindex");
	if ( !index->if_name && !index->if_index )
		errx(1, "no loopback interface was found");
	if ( !index->if_name )
		errx(1, "first interface had no name");
	if ( !index->if_index )
		errx(1, "first interface had no index");
	return 0;
}
