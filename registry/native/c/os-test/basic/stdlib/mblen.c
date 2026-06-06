/* Test whether a basic mblen invocation works. */

#include <stdlib.h>

#include "../basic.h"

int main(void)
{
	errno = 0;
	int ret = mblen("xy", 2);
	if ( ret < 0 )
		err(1, "mblen");
	else if ( ret != 1 ) 
		err(1, "mblen was %d, not %d", ret, 1);
	return 0;
}
