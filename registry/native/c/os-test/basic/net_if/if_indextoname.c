/* Test whether a basic if_indextoname invocation works. */

#include <net/if.h>
#include <string.h>

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
	char buf[IF_NAMESIZE];
	char* result = if_indextoname(index->if_index, buf);
	if ( !result )
		err(1, "if_indextoname");
	if ( result != buf )
		errx(1, "if_indextoname did not return buf");
	if ( strcmp(result, index->if_name) != 0 )
		errx(1, "if_indextoname returned wrong name");
	return 0;
}
