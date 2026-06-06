/*[XSI]*/
/* Test whether a basic hcreate invocation works. */

#include <search.h>

#include "../basic.h"

int main(void)
{
	if ( !hcreate(1024) )
		err(1, "hcreate");
	return 0;
}
