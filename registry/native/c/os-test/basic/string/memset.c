/* Test whether a basic memset invocation works. */

#include <string.h>

#include "../basic.h"

int main(void)
{
	char buf[8];
	void* ptr = memset(buf, 'x', 8);
	if ( ptr != buf )
		errx(1, "memset did not return buf");
	for ( size_t i = 0; i < 8; i++ )
		if ( buf[i] != 'x' )
			err(1, "buf[%zu] != 'x'", i);
	return 0;
}
