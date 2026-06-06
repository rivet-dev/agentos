/* Test whether a basic fmemopen invocation works. */

#include <stdio.h>

#include "../basic.h"

int main(void)
{
	char buf[] = "foo";
	FILE* fp = fmemopen(buf, sizeof(buf), "r");
	if ( !fp )
		err(1, "fmemopen");
	return 0;
}
