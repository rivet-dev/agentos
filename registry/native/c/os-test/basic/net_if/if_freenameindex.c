/* Test whether a basic if_freenameindex invocation works. */

#include <net/if.h>

#include "../basic.h"

int main(void)
{
	struct if_nameindex* index = if_nameindex();
	if ( !index )
		err(1, "if_nameindex");
	if_freenameindex(index);
	return 0;
}
